Solutions to the cool challenges at https://fly.io/dist-sys, which are built on top of Jepsen's [Maelstrom](https://github.com/jepsen-io/maelstrom)

To run:

1. `brew install openjdk graphviz gnuplot`
2. [Download Maelstrom](https://github.com/jepsen-io/maelstrom/releases/tag/v0.2.3)
3. Run `maelstrom test -w <WORKLOAD> --bin ./target/debug/<WORKLOAD> --node-count 1 --time-limit 10 --log-stderr` - change parameters as necessary

### Recommended parameters

* echo: `maelstrom test -w echo --bin ./target/debug/echo --node-count 1 --time-limit 10 --log-stderr`
* unique-id: `maelstrom test -w unique-ids --bin ./target/debug/unique-id --time-limit 30 --rate 1000 --node-count 3 --availability total --nemesis partition`
