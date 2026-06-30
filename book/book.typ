#import "dag.typ": dag, dag-linear, node, edge

#let colors = (
  up: rgb("#fbf8cc"),
  down: rgb("#fde4cf"),
  range: rgb("#ffcfd2"),
)

#let n(id, label, level, pos: none) = node(id, text(font: "Courier New", size: 9pt, label), level, pos: pos)

= DAG Operations

Let's cover some elementary DAG operations and how they look on an example.
Here's the history we'll be working with:

#let dag1-nodes = (
  n("z",   "z", 0),
  n("a",   "a", 1),
  n("b",   "b", 1),
  n("c",   "c", 1),
  n("u",   "u", 2),
  n("v",   "v", 2),
  n("w",   "w", 2),
  n("p",   "p", 3),
  n("q",   "q", 3),
  n("out", "@", 4),
  n("feat", "m", 4),
)
#let dag1-edges = (
  edge("z", "a"),
  edge("z", "b"),
  edge("z", "c"),
  edge("a", "u"),
  edge("b", "u"),
  edge("b", "v"),
  edge("c", "v"),
  edge("c", "w"),
  edge("u", "p"),
  edge("v", "p"),
  edge("v", "q"),
  edge("w", "q"),
  edge("p", "out"),
  edge("q", "out"),
  edge("q", "feat"),
)

#let result(query, ids, color: colors.range) = {
  let highlights = (:)
  for id in ids { highlights.insert(id, color) }
  align(center, {
    block(raw(query))
    dag(nodes: dag1-nodes, edges: dag1-edges, x-spacing: 1.0, highlights: highlights)
  })
}

#align(center,
  dag(
    nodes: dag1-nodes,
    edges: dag1-edges,
    x-spacing: 1.0,
    highlights: (),
  )
)

Typically, we would probably render a DAG upside-down from this, but doing it this way mirrors the display of `jj log`, and so we will display it upside-down.
Just know that if you show this to someone outside of a version control context, they might find this presentation confusing.

The other orientation of a DAG that will be useful for us to keep in mind is exactly the above, but rotated clockwise:

#align(center,
  dag(
    nodes: dag1-nodes,
    edges: dag1-edges,
    x-spacing: 1.0,
    highlights: (),
    direction: "ltr",
  )
)

The reason this orientation is useful is that it mirrors the direction and order of queries that we will see later, like `a::`, `::b`, and `a::b`.
If you are ever confused about the canonical textual order of things, take the `jj log` output and rotate it 90 degrees clockwise in your brain.
This is, of course, the natural way to do it, since time flows left-to-right, but for me it was helpful to make this explicit for myself.

== Revision References

The revset language is expressed over sets of vertices (or, revisions).
The name of a revision resolves to that revision:

#block(breakable: false, grid(
  columns: (1fr, 1fr),
  result("v", ("v",)),
  result("w", ("w",)),
))

To be precise, there is a priority order in which `jj` tries to resolve symbols.
Starting from the highest priority:

- tag name (from Git),
- bookmark name,
- git ref,
- commit ID or change ID.

== Set Operations

We can perform union (`|`), intersection (`&`), and difference (`~`) operations.

#block(breakable: false, grid(
  columns: (1fr, 1fr, 1fr),
  result("b | w", ("b", "w"), color: colors.up),
  result("(b | w) & (p | w)", ("w"), color: colors.down),
  result("(b | w) ~ w", ("b")),
))

== Ancestors and Descendants

One important operation over a DAG is the *closure* of a set in some direction.
In our context, we will be calling these the *ancestors*, written `::x` and *descendants*, written `x::`, of a given set `x`.

#block(breakable: false, grid(
  columns: (1fr, 1fr),
  result("::p", ("p", "u", "v", "a", "b", "c", "z"),   color: colors.down),
  result("b::", ("b", "u", "v", "p", "q", "m", "out"), color: colors.up),
  result("::(a | v)", ("a", "v", "a", "b", "c", "z"),   color: colors.down),
  result("(a | v)::", ("a", "v", "u", "p", "out", "q", "m"),   color: colors.up),
))

`jj` optimizes primarily for ancestor queries being fast.
This is because a revision depends on its ancestors; not its descendants, and so from the point of view of a given revision, it's most important to be able to quickly get information about its ancestors.

Let's now look at how `jj` actually executes these kinds of queries.

The `jj` database stores commits in a structure called the *commit index*.
This index stores all the commits in a linear order, where every commit is given a numeric identifier called its *global commit number* (GCN).
These identifiers aren't stable; they're not guaranteed to be the same on different invocations, and they're not exposed to users.
The ordering obeys the invariant that the ordering of commits by their global commit number is a topological sort of the DAG:
if `a` is an ancestor of `b`, then `a` has a lower GCN than `b`.

The commit index provides fast access to iterate over commits in descending GCN order, meaning we are guaranteed that any child will always be visited before its parents.
This gives us a simple ancestry algorithm:

```python
def ancestors(initial):
  let result = initial
  let scan_begin = max(initial.map(|i| i.gcn))
  let parents = set(initial.flat_map(|i| i.parents))
  for rev in scan(scan_begin..):
    if rev in parents:
      result.push(rev)
      parents.extend(rev.parents)
  return result
```

== DAG Range

One important operation over DAGs is *range*.

#block(breakable: false, grid(
  columns: (1fr, 1fr),
  result("b::", ("b", "u", "v", "p", "q", "m", "out"), color: colors.up),
  result("::p", ("p", "u", "v", "a", "b", "c", "z"),   color: colors.down),
))

= `b::p`

#align(center, result("b::p", ("b", "u", "v", "p")))

= Operators

- `a::`
- `::b`
- `a::b`

= Identities

- `::a | ::b` $equiv$ `::(a | b)`
- `a:: | b::` $equiv$ `(a | b)::`
