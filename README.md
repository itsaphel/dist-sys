This repository contains my implementation for various distributed systems algorithms, which are tested using
Jepsen's [Maelstrom](https://github.com/jepsen-io/maelstrom). I found these cool challenges on https://fly.io/dist-sys,
and they're also described in the Maelstrom repo [here](https://github.com/jepsen-io/maelstrom/tree/main/doc).

# Setup

1. `brew install openjdk graphviz gnuplot`
2. [Download Maelstrom](https://github.com/jepsen-io/maelstrom/releases/tag/v0.2.3) and extract into the root of this
   git repository
3. Run
   `maelstrom/maelstrom test -w <WORKLOAD> --bin ./target/debug/<WORKLOAD> --node-count 1 --time-limit 10 --log-stderr` -
   change parameters as necessary

You can add `maelstrom/maelstrom` to your path, or move it to a directory in your path, to simplify the run command to
just `maelstrom`.

# Workloads

## Echo

* Test command: `maelstrom test -w echo --bin ./target/debug/echo --node-count 1 --time-limit 10 --log-stderr`

## Unique IDs

* Test command:
  `maelstrom test -w unique-ids --bin ./target/debug/unique-id --time-limit 30 --rate 1000 --node-count 3 --availability total --nemesis partition`

The implementation is UUID v7. Throughput (as reported by Maelstrom) is 850 req/s with a latency of around 1ms, on my
Mac. This is similar to a per-node globally incrementing counter approach. Perhaps the bottleneck is my Maelstrom
arguments or my laptop, rather than the algorithm itself. It'd also be cool to try out something
like [Twitter's Snowflake](https://github.com/twitter-archive/snowflake/tree/b3f6a3c6ca8e1b6847baa6ff42bf72201e2c2231),
which reports a minimum 10k ids/sec/process performance.

## Broadcast

* Single-node test:
  `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 1 --time-limit 20 --rate 10`
* Multi-node test: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 5 --time-limit 20 --rate 10`
* Multi-node with network partitions:
  `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 5 --time-limit 20 --rate 10 --nemesis partition`
* Performance I:
`maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 25 --time-limit 20 --rate 100 --latency 100`

Notes when implementing multi-node broadcast:
* Going from tokio's Mutex to std's Mutex increased throughput slightly, but not massively. Main throughput increase was observed in startup time.
* Switching from `std::sync::Mutex` to `std::sync::RwLock` decreased performance *slightly*, but was negligible. This
  was a surprising result, so I reckon the difference might just be due to random variance and the test should be
  repeated a couple times. Otherwise, it could be due to the specifics of how the test is ran in Maelstrom (ie: how
  parallelised the nodes really are). Also, it could be bottlenecked by the `rate`.

### Performance

The first sub-challenge sets the network to 25 nodes with 100ms injected latency to simulate a slow network. The goal
is:

* Messages per operation below 30
* Median latency below 400ms
* Maximum latency below 600ms

Under a grid topology, my existing solution for multi-broadcast achieves:

* Messages per operation: 48.275764
* Median latency: 1048ms
* Max latency: 2324ms

The key to improving performance further is to change the topology. A grid topology results in a lot of write
amplification. In a 3x3 grid:

```
n1 n2 n3
n4 n5 n6
n7 n8 n9
```

note that a message sent to n5 propagates to n2 and n4, who then send the message to their neighbours, so n1 receives
the same message twice. This quickly gets worse in larger grids.

A line topology is optimal for decreasing `messages-per-op`, but increases latency substantially and, in the presence of
failures, also is quite bad for fault-tolerance (note that, in pathological cases, nodes on the tails of the line are
particularly susceptible to failures).

To meet the requirements of challenge 3d, a tree-based topology is sufficient. Maelstrom has support for various tree
topologies which can be enabled with the `--topology` flag. With `--topology tree4` the metrics are much lower:

* Messages per operation: 24.052805
* Median latency: 365ms
* Max latency: 516ms

### Fault-tolerance

The key to achieving fault-tolerance, in my implementation, is retries. If a communication fails, the message is added
to a queue and a single background thread attempts to resend the message until it is acknowledged.

To prevent a ton of OS threads being spawned, a single worker is used to process the queue, rather than the handler
thread continuing indefinitely until the message is acknowledged. Further, to prevent a single failing node from
blocking the entire queue, a message is popped from the head during retry and is appended to the tail of the queue if it
fails again.

Issues with the implementation:

* The worker is currently a tight loop with no sleep. A cooldown is perhaps good to avoid hitting failed nodes too hard.
* Consider [circuit breakers](https://martinfowler.com/bliki/CircuitBreaker.html): if a particular node is down, it's
  not particularly helpful for more requests to continue being sent to that node. For example, if the cause of the
  failure is too much traffic and the node running out of resources, more traffic isn't going to help the situation.
  More concretely to Maelstrom's metrics, circuit breakers should decrease the `messages-per-op` in the presence of
  failures without increasing latency much.

  I tried to implement circuit breakers in my implementation (using the [failsafe](https://crates.io/crates/failsafe)
  crate), but it didn't seem to work well. I suspect that crate is not designed for failures that last sub-1s and may go
  from failing-healthy-failing in less than a second, which is a possible scenario in the Maelstrom workload.

## CRDTs

One downside of broadcast is that it's quite chatty. What if we could send a batch of messages instead? Continuing down
this line of thought, we end up at CRDTs. Conflict-free replicated data types (CRDTs) are any data structure that can be
replicated across multiple nodes, with an eventual consistency model that resolves conflicts in some manner.

To resolve conflicts, we can either make the operations commutative (applying `a` then `b` is the same as applying `b`
then `a`), or avoid conflicts in the first place by ensuring the sequence of events is replicated in-order (we'll
discuss this problem in the Kafka section below).

When batching together messages, we get lower `messages-per-op`, in exchange for latency potentially being higher.

## Kafka

The Kafka challenge is about replicating a log across nodes.

Each log is identified by a key (like a kafka topic) and contains a bunch of messages which are identified by integer
offsets. Two properties are verified by the checker:

* Offsets are monotonically increasing: newer messages have a larger offset number
* No lost writes: If a node sees offset `10`, it must've also seen offset `5`

The system is eventually consistent with no bound on replication lag; nodes do not need to provide newly sent messages
immediately.

### Single-node replicated log design

Access patterns:

* `send` needs to take a topic ID and a message. The message needs to be appended to the log of the topic. It needs to
  return an offset under which the message can be found.
* `poll` can request messages from multiple topics. It provides a topic ID and a minimum offset number. For each topic,
  the response needs to contain messages >= that offset. However, note that the server is allowed to limit the amount of
  messages to N per topic, which is useful if there is a large number of messages.
* `commit_offsets` is used to indicate a client has processed messages up to (and including) the offset. Multiple topics
  can be referenced in the request.
* `list_committed_offsets` is used when the client wants to know what offset, per topic, has been processed, so it knows
  where to start consuming from. The number returned should be the last committed offset.

Based on these access patterns a binary tree or btree structure makes sense for quick lookups of messages per topic, by
offset (which is the key).
