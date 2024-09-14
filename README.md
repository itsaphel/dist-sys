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

Test command: `maelstrom test -w broadcast --bin ./target/debug/broadcast --node-count 1 --time-limit 20 --rate 10`
