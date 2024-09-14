Solutions to the cool challenges at https://fly.io/dist-sys, which are built on top of Jepsen's [Maelstrom](https://github.com/jepsen-io/maelstrom)

To run:

1. `brew install openjdk graphviz gnuplot`
2. [Download Maelstrom](https://github.com/jepsen-io/maelstrom/releases/tag/v0.2.3)
3. Run `maelstrom test -w <WORKLOAD> --bin ./target/debug/<WORKLOAD> --node-count 1 --time-limit 10 --log-stderr` - change parameters as necessary

## Workloads

### Echo

Test command: `maelstrom test -w echo --bin ./target/debug/echo --node-count 1 --time-limit 10 --log-stderr`

### Unique IDs

Test command: `maelstrom test -w unique-ids --bin ./target/debug/unique-id --time-limit 30 --rate 1000 --node-count 3 --availability total --nemesis partition`

The implementation is UUID v7. Throughput (as reported by Maelstrom) is 850 req/s with a latency of around 1ms, on my Mac. This is similar to a per-node globally incrementing counter approach. Perhaps the bottleneck is my Maelstrom arguments and my laptop, rather than the algorithm itself. It'd be cool to try out something like [Twitter's Snowflake](https://github.com/twitter-archive/snowflake/tree/b3f6a3c6ca8e1b6847baa6ff42bf72201e2c2231), which reports a minimum 10k ids/process performance.

### Broadcast

Single-node test: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 1 --time-limit 20 --rate 10`  
Multi-node test: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 5 --time-limit 20 --rate 10`  
Multi-node with network partitions: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 5 --time-limit 20 --rate 10 --nemesis partition`

Notes when implementing multi-node broadcast:
* Going from tokio's Mutex to std's Mutex increased throughput slightly, but not massively. Main throughput increase was observed in startup time.
* Switching from std::sync::Mutex to std::sync::RwLock decreased performance *slightly*, but was negligible. This was a surprising result, so I reckon the difference might just be due to random variance and the test should be repeated a couple times. Otherwise, it could be due to the specifics of how the test is ran in Maelstrom (ie: how parallelised the nodes really are). Also, it could be bottlenecked by the `rate`.