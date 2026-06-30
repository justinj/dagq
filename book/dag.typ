// dag.typ — helpers for drawing level-ordered DAGs with cetz

#import "@preview/cetz:0.3.4": canvas, draw

// --- internal vector math ---

#let _vsub(a, b) = (a.at(0) - b.at(0), a.at(1) - b.at(1))
#let _vadd(a, b) = (a.at(0) + b.at(0), a.at(1) + b.at(1))
#let _vscale(v, s) = (v.at(0) * s, v.at(1) * s)
#let _vlen(v) = calc.sqrt(v.at(0) * v.at(0) + v.at(1) * v.at(1))
#let _vnorm(v) = {
  let l = _vlen(v)
  if l == 0 { (0.0, 0.0) } else { (v.at(0) / l, v.at(1) / l) }
}

// --- public constructors ---

/// Create a node dict.
///
/// - id:    unique string identifier
/// - label: content to render at the node position
/// - level: integer depth (0 = bottom)
/// - pos:   optional slot override within the level (default: order in the nodes array)
#let node(id, label, level, pos: none) = {
  let d = (id: id, label: label, level: level)
  if pos != none { d.insert("pos", pos) }
  d
}

/// Create an edge pair.
#let edge(from, to) = (from, to)

// --- layout ---

// Assign (x, y) coordinates to each node id.
// Nodes at the same level are spread left-to-right from x = 0.
// Level 0 is at the bottom; each subsequent level is y-spacing higher.
#let _layout(nodes, x-spacing, y-spacing) = {
  // bucket nodes by level (preserving insertion order)
  let by-level = (:)
  for n in nodes {
    let k = str(n.level)
    if k not in by-level { by-level.insert(k, ()) }
    let arr = by-level.at(k)
    arr.push(n)
    by-level.insert(k, arr)
  }

  let positions = (:)
  for (k, lvl-nodes) in by-level {
    let lvl = int(k)
    let count = lvl-nodes.len()
    for (i, n) in lvl-nodes.enumerate() {
      let slot = if "pos" in n { n.pos } else { i }
      let x = slot * x-spacing
      let y = lvl * y-spacing
      positions.insert(n.id, (x, y))
    }
  }
  positions
}

// --- topological sort ---

// Kahn's algorithm. Returns node ids oldest-first (sources before descendants).
#let topo-sort(node-ids, edges) = {
  let in-deg = (:)
  let adj = (:)
  for id in node-ids {
    in-deg.insert(id, 0)
    adj.insert(id, ())
  }
  for e in edges {
    let from = e.at(0)
    let to = e.at(1)
    let arr = adj.at(from)
    arr.push(to)
    adj.insert(from, arr)
    in-deg.insert(to, in-deg.at(to) + 1)
  }
  let result = ()
  for _ in range(node-ids.len()) {
    for id in node-ids {
      if id not in result and in-deg.at(id) == 0 {
        result.push(id)
        for nb in adj.at(id) {
          in-deg.insert(nb, in-deg.at(nb) - 1)
        }
        break
      }
    }
  }
  result
}

// --- main functions ---

/// Draw a level-ordered DAG.
///
/// Parameters:
/// - nodes:       array of node dicts (use `node(...)`)
/// - edges:       array of edge pairs (use `edge(...)`)
/// - highlights:  dict mapping node ids to fill colors, e.g. ("a": yellow, "b": red)
/// - node-radius: invisible circle radius — controls padding and edge clearance
/// - x-spacing:   gap between nodes in the same level (across-flow axis)
/// - y-spacing:   gap between levels (along-flow axis)
/// - direction:   "ttb" (top-to-bottom, default) or "ltr" (left-to-right)
/// - edge-stroke: stroke for edges
/// - unit:        cetz canvas length unit
#let dag(
  nodes: (),
  edges: (),
  highlights: (:),
  node-radius: 0.35,
  x-spacing: 1.6,
  y-spacing: 1.4,
  direction: "ttb",
  edge-stroke: black + 0.7pt,
  unit: 1cm,
) = {
  let positions = _layout(nodes, x-spacing, y-spacing)
  let positions = if direction == "ltr" {
    let swapped = (:)
    for (id, p) in positions { swapped.insert(id, (p.at(1), -p.at(0))) }
    swapped
  } else { positions }
  let r = node-radius

  canvas(length: unit, {
    // Edges behind nodes
    for e in edges {
      let p1 = positions.at(e.at(0))
      let p2 = positions.at(e.at(1))
      let d = _vnorm(_vsub(p2, p1))
      let start = _vadd(p1, _vscale(d, r))
      let end = _vsub(p2, _vscale(d, r + 0.05))
      draw.line(start, end, stroke: edge-stroke, mark: (end: (symbol: ">", fill: black), size: 0.18))
    }

    // Circle masks the edge line, label sits on top; highlighted nodes get a fill color
    for n in nodes {
      let pos = positions.at(n.id)
      let fill = if n.id in highlights { highlights.at(n.id) } else { white }
      draw.circle(pos, radius: r, fill: fill, stroke: none)
      draw.content(pos, box(n.label))
    }
  })
}

/// Draw a topological linearization of a DAG.
/// Nodes are placed in a horizontal line in the given order.
/// Every edge becomes a backward arc above the line — the fact that
/// all arcs point the same way is what makes it a valid topological sort.
///
/// Parameters:
/// - nodes, edges: same as dag()
/// - highlights:   dict mapping node ids to fill colors — same as dag()
/// - order:        array of node ids in the desired left-to-right sequence
/// - node-radius:  padding / edge clearance
/// - x-spacing:    horizontal distance between adjacent nodes
/// - arc-factor:   arc height = edge-span * arc-factor
/// - edge-stroke, unit: same as dag()
#let dag-linear(
  nodes: (),
  edges: (),
  highlights: (:),
  order: (),
  node-radius: 0.35,
  x-spacing: 1.4,
  arc-factor: 0.45,
  edge-stroke: black + 0.7pt,
  unit: 1cm,
) = {

  let pos = (:)
  for (i, id) in order.enumerate() {
    pos.insert(id, (float(i) * x-spacing, 0.0))
  }

  let node-map = (:)
  for n in nodes { node-map.insert(n.id, n) }

  canvas(length: unit, {
    for e in edges {
      let p1 = pos.at(e.at(0))
      let p2 = pos.at(e.at(1))
      let mx = (p1.at(0) + p2.at(0)) / 2
      let h = calc.abs(p2.at(0) - p1.at(0)) * arc-factor
      let ctrl = (mx, h)
      let t1 = _vnorm(_vsub(ctrl, p1))
      let t2 = _vnorm(_vsub(p2, ctrl))
      let start = _vadd(p1, _vscale(t1, node-radius))
      let end = _vsub(p2, _vscale(t2, node-radius + 0.05))
      draw.bezier(start, end, ctrl, stroke: edge-stroke, mark: (end: (symbol: ">", fill: black), size: 0.18))
    }

    for id in order {
      let p = pos.at(id)
      let fill = if id in highlights { highlights.at(id) } else { white }
      draw.circle(p, radius: node-radius, fill: fill, stroke: none)
      draw.content(p, box(node-map.at(id).label))
    }
  })
}
