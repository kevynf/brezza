use crate::model::{CommitSummary, GraphEdge, GraphRow, Oid, UNCOMMITTED};
const UNASSIGNED: u16 = u16::MAX - 1;

#[derive(Clone, Copy)]
struct Lane {
    id: Oid,
    color: u16,
}

pub fn layout(commits: &[CommitSummary], head: Option<Oid>) -> Vec<GraphRow> {
    let working = head.map(|h| (Oid::null(h.kind()), vec![h]));
    let nodes: Vec<(Oid, &[Oid])> = working
        .iter()
        .map(|(id, parents)| (*id, parents.as_slice()))
        .chain(commits.iter().map(|c| (c.id, c.parents.as_slice())))
        .collect();

    let mut lanes: Vec<Lane> = Vec::new();
    let mut rows = Vec::with_capacity(nodes.len());
    for (r, &(id, parents)) in nodes.iter().enumerate() {
        let node = match lanes.iter().position(|l| l.id == id) {
            Some(i) => i,
            None => {
                lanes.push(Lane {
                    id,
                    color: UNASSIGNED,
                });
                lanes.len() - 1
            }
        };
        if lanes[node].color == UNASSIGNED || (lanes[node].color == UNCOMMITTED && r > 0) {
            lanes[node].color = if working.is_some() && r == 0 {
                UNCOMMITTED
            } else {
                free_color(&lanes)
            };
        }
        let color = lanes[node].color;

        let mut next: Vec<(Lane, usize, bool)> = Vec::new();
        let mut merges = Vec::new();
        for (i, lane) in lanes.iter().enumerate() {
            if i != node {
                next.push((*lane, i, false));
                continue;
            }
            for (k, &parent) in parents.iter().enumerate() {
                if k == 0 {
                    next.push((Lane { id: parent, color }, i, false));
                } else if let Some(existing) = lanes.iter().find(|l| l.id == parent) {
                    merges.push((*existing, i, true));
                } else {
                    let lane = Lane {
                        id: parent,
                        color: UNASSIGNED,
                    };
                    merges.push((lane, i, false));
                }
            }
        }
        next.extend(merges);

        let next_id = nodes.get(r + 1).map(|n| n.0);
        let mut out: Vec<Lane> = Vec::new();
        let mut edges = Vec::new();
        for (mut lane, from, joins) in next {
            let target = (joins || Some(lane.id) == next_id)
                .then(|| out.iter().position(|l| l.id == lane.id))
                .flatten();
            let to = match target {
                Some(t) => {
                    if out[t].color == UNCOMMITTED {
                        out[t].color = lane.color;
                    }
                    t
                }
                None => {
                    if lane.color == UNASSIGNED {
                        lane.color = free_color(&out);
                    }
                    out.push(lane);
                    out.len() - 1
                }
            };
            edges.push(GraphEdge {
                from: from as u16,
                to: to as u16,
                color: lane.color,
            });
        }
        rows.push(GraphRow {
            lane: node as u16,
            color,
            head: head == Some(id),
            edges,
        });
        lanes = out;
    }
    rows
}

pub fn lanes(rows: &[GraphRow]) -> u16 {
    rows.iter()
        .flat_map(|row| {
            row.edges
                .iter()
                .flat_map(|e| [e.from, e.to])
                .chain([row.lane])
        })
        .max()
        .unwrap_or(0)
}

fn free_color(lanes: &[Lane]) -> u16 {
    (0..)
        .find(|c| !lanes.iter().any(|l| l.color == *c))
        .unwrap_or(0)
}
