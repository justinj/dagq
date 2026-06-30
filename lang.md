We only care about a simple set of operators from the jj revset syntax:

* literals (a b x y),
* set ops (a | b, a & b, a ~ b),
* descendant queries (a::),
* ancestor queries (::a),
* range queries (a::b),
* functions (heads(a::))

write a simple recursive descent parser that makes an enum ast.

consult https://docs.jj-vcs.dev/latest/revsets/ for information about things
like precedence.

there should be a stringifier, and we should have tests that we round-trip expressions appropriately.
