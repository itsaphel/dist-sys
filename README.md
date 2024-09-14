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

Test command: `maelstrom test -w echo --bin ./target/debug/echo --node-count 1 --time-limit 10 --log-stderr`

## Unique IDs

Test command: `maelstrom test -w unique-ids --bin ./target/debug/unique-id --time-limit 30 --rate 1000 --node-count 3 --availability total --nemesis partition`

The implementation is UUID v7. Throughput (as reported by Maelstrom) is 850 req/s with a latency of around 1ms, on my Mac. This is similar to a per-node globally incrementing counter approach. Perhaps the bottleneck is my Maelstrom arguments and my laptop, rather than the algorithm itself. It'd be cool to try out something like [Twitter's Snowflake](https://github.com/twitter-archive/snowflake/tree/b3f6a3c6ca8e1b6847baa6ff42bf72201e2c2231), which reports a minimum 10k ids/process performance.

## Broadcast

Single-node test: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 1 --time-limit 20 --rate 10`  
Multi-node test: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 5 --time-limit 20 --rate 10`  
Multi-node with network partitions:
`maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 5 --time-limit 20 --rate 10 --nemesis partition`  
Performance I:
`maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 25 --time-limit 20 --rate 100 --latency 100`

Notes when implementing multi-node broadcast:
* Going from tokio's Mutex to std's Mutex increased throughput slightly, but not massively. Main throughput increase was observed in startup time.
* Switching from std::sync::Mutex to std::sync::RwLock decreased performance *slightly*, but was negligible. This was a surprising result, so I reckon the difference might just be due to random variance and the test should be repeated a couple times. Otherwise, it could be due to the specifics of how the test is ran in Maelstrom (ie: how parallelised the nodes really are). Also, it could be bottlenecked by the `rate`.

### Performance

The first sub-challenge sets the network to 25 nodes with 100ms injected latency to simulate a slow network. The goal
is:

* Messages per operation below 30
* Median latency below 400ms
* Maximum latency below 600ms

My current solution for multi-broadcast, described above, achieves:

* Messages per operation: 48.275764
* Median latency: 2527ms
* Max latency: 8775ms
