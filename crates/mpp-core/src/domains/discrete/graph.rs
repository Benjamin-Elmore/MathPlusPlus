//! Finite simple undirected graphs as values: an edge list `((u, v), …)` on vertices
//! 1 … n, or a symmetric 0/1 adjacency matrix. Degrees, adjacency and Laplacian
//! matrices (L = D − A), connectivity, the tree test (connected with |E| = |V| − 1)
//! and Kirchhoff's matrix-tree theorem for the number of spanning trees.

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::domains::fieldkit::{domain_err, matrix_rows};
use crate::domains::util::int_arg;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// Most vertices.
pub const MAX_VERTICES: usize = 64;

/// A simple undirected graph on vertices `0 … n−1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    n: usize,
    adj: Vec<Vec<bool>>,
}

impl Graph {
    /// A graph from 0-based edges; loops and repeated edges are errors.
    pub fn from_edges(n: usize, edges: &[(usize, usize)]) -> MathResult<Graph> {
        if n == 0 || n > MAX_VERTICES {
            return Err(MathError::TooLarge(format!(
                "a graph on {n} vertices (1 … {MAX_VERTICES})"
            )));
        }
        let mut adj = vec![vec![false; n]; n];
        for &(u, v) in edges {
            if u >= n || v >= n {
                return Err(domain_err(
                    "graph",
                    format!("the vertex {} is out of range", u.max(v) + 1),
                ));
            }
            if u == v {
                return Err(domain_err(
                    "graph",
                    format!("the loop at {} is not allowed in a simple graph", u + 1),
                ));
            }
            if std::mem::replace(&mut adj[u][v], true) {
                return Err(domain_err(
                    "graph",
                    format!("the edge ({}, {}) is listed twice", u + 1, v + 1),
                ));
            }
            adj[v][u] = true;
        }
        Ok(Graph { n, adj })
    }

    /// The complete graph K_n.
    pub fn complete(n: usize) -> MathResult<Graph> {
        let edges: Vec<(usize, usize)> = (0..n)
            .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
            .collect();
        Graph::from_edges(n, &edges)
    }

    /// Number of vertices.
    pub fn order(&self) -> usize {
        self.n
    }

    /// Number of edges.
    pub fn size(&self) -> usize {
        self.degrees().iter().sum::<usize>() / 2
    }

    /// The degree of each vertex.
    pub fn degrees(&self) -> Vec<usize> {
        self.adj
            .iter()
            .map(|r| r.iter().filter(|&&b| b).count())
            .collect()
    }

    /// The adjacency matrix as integers.
    pub fn adjacency(&self) -> Vec<Vec<i64>> {
        self.adj
            .iter()
            .map(|r| r.iter().map(|&b| i64::from(b)).collect())
            .collect()
    }

    /// The Laplacian `D − A`.
    pub fn laplacian(&self) -> Vec<Vec<i64>> {
        let deg = self.degrees();
        (0..self.n)
            .map(|i| {
                (0..self.n)
                    .map(|j| {
                        if i == j {
                            deg[i] as i64
                        } else {
                            -i64::from(self.adj[i][j])
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// True when every vertex is reachable from vertex 0.
    pub fn is_connected(&self) -> bool {
        let mut seen = vec![false; self.n];
        let mut stack = vec![0];
        seen[0] = true;
        while let Some(u) = stack.pop() {
            for v in 0..self.n {
                if self.adj[u][v] && !seen[v] {
                    seen[v] = true;
                    stack.push(v);
                }
            }
        }
        seen.iter().all(|&s| s)
    }

    /// Connected with |E| = |V| − 1.
    pub fn is_tree(&self) -> bool {
        self.size() + 1 == self.n && self.is_connected()
    }

    /// Kirchhoff: any cofactor of the Laplacian (exact, fraction-free Bareiss).
    pub fn spanning_trees(&self) -> BigInt {
        if self.n == 1 {
            return BigInt::from(1);
        }
        let l = self.laplacian();
        let m: Vec<Vec<BigInt>> = l[1..]
            .iter()
            .map(|r| r[1..].iter().map(|&x| BigInt::from(x)).collect())
            .collect();
        bareiss_det(m)
    }
}

/// The determinant of an integer matrix by fraction-free Bareiss elimination.
pub fn bareiss_det(mut m: Vec<Vec<BigInt>>) -> BigInt {
    let n = m.len();
    let mut sign = 1;
    let mut prev = BigInt::from(1);
    for k in 0..n {
        if m[k][k].is_zero() {
            match (k + 1..n).find(|&r| !m[r][k].is_zero()) {
                Some(r) => {
                    m.swap(k, r);
                    sign = -sign;
                }
                None => return BigInt::zero(),
            }
        }
        for i in k + 1..n {
            for j in k + 1..n {
                let v = (&m[i][j] * &m[k][k] - &m[i][k] * &m[k][j]) / &prev;
                m[i][j] = v;
            }
        }
        prev = m[k][k].clone();
    }
    if n == 0 {
        return BigInt::from(1);
    }
    let d = m[n - 1][n - 1].clone();
    if sign < 0 {
        -d
    } else {
        d
    }
}

fn vertex(op: &str, e: &Expr) -> MathResult<usize> {
    int_arg(op, e)?
        .to_usize()
        .filter(|&v| (1..=MAX_VERTICES).contains(&v))
        .map(|v| v - 1)
        .ok_or_else(|| domain_err(op, format!("{e} is not a vertex 1 … {MAX_VERTICES}")))
}

/// Reads a graph: an adjacency matrix, or an edge list with an optional vertex count.
pub fn graph_arg(op: &str, e: &Expr, n: Option<usize>) -> MathResult<Graph> {
    if let ExprNode::Matrix { .. } = e.node() {
        let rows = matrix_rows(op, e)?;
        let size = rows.len();
        if rows.iter().any(|r| r.len() != size) {
            return Err(domain_err(op, "an adjacency matrix must be square"));
        }
        let mut edges = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            for (j, x) in row.iter().enumerate() {
                let v = int_arg(op, x)?;
                let sym = int_arg(op, &rows[j][i])?;
                if v != sym || !(v.is_zero() || v == BigInt::from(1)) || (i == j && !v.is_zero()) {
                    return Err(domain_err(op, "an adjacency matrix must be symmetric with 0/1 entries and a zero diagonal"));
                }
                if j > i && v == BigInt::from(1) {
                    edges.push((i, j));
                }
            }
        }
        return Graph::from_edges(size, &edges);
    }
    let ExprNode::Tuple(items) = e.node() else {
        return Err(domain_err(
            op,
            format!("{e} is not a graph (an edge list or an adjacency matrix)"),
        ));
    };
    let mut edges = Vec::with_capacity(items.len());
    for it in items {
        match it.node() {
            ExprNode::Tuple(uv) if uv.len() == 2 => {
                edges.push((vertex(op, &uv[0])?, vertex(op, &uv[1])?))
            }
            _ => return Err(domain_err(op, format!("{it} is not an edge (u, v)"))),
        }
    }
    let max = edges.iter().map(|&(u, v)| u.max(v) + 1).max().unwrap_or(0);
    let n = n.unwrap_or(max);
    if n < max {
        return Err(domain_err(
            op,
            format!("the edges use vertex {max} but the graph has {n} vertices"),
        ));
    }
    Graph::from_edges(n, &edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn small_graphs() {
        // K₄: 16 spanning trees (Cayley 4^{4−2}); C₄: 4; a path is a tree.
        let k4 = Graph::complete(4).unwrap();
        assert_eq!(k4.spanning_trees(), BigInt::from(16));
        assert_eq!(k4.degrees(), vec![3, 3, 3, 3]);
        let c4 = Graph::from_edges(4, &[(0, 1), (1, 2), (2, 3), (3, 0)]).unwrap();
        assert_eq!(c4.spanning_trees(), BigInt::from(4));
        assert!(!c4.is_tree());
        let path = Graph::from_edges(4, &[(0, 1), (1, 2), (2, 3)]).unwrap();
        assert!(path.is_tree());
        assert_eq!(path.laplacian()[1], vec![-1, 2, -1, 0]);
        let disconnected = Graph::from_edges(4, &[(0, 1), (2, 3)]).unwrap();
        assert_eq!(disconnected.spanning_trees(), BigInt::zero());
        assert!(Graph::from_edges(3, &[(0, 0)]).is_err());
        assert!(Graph::from_edges(3, &[(0, 1), (1, 0)]).is_err());
        // Petersen graph: 2000 spanning trees (OEIS A000000-style classic; Biggs, Algebraic Graph Theory).
        let outer: Vec<(usize, usize)> = (0..5).map(|i| (i, (i + 1) % 5)).collect();
        let spokes: Vec<(usize, usize)> = (0..5).map(|i| (i, i + 5)).collect();
        let inner: Vec<(usize, usize)> = (0..5).map(|i| (5 + i, 5 + (i + 2) % 5)).collect();
        let edges: Vec<(usize, usize)> = outer.into_iter().chain(spokes).chain(inner).collect();
        let petersen = Graph::from_edges(10, &edges).unwrap();
        assert_eq!(petersen.spanning_trees(), BigInt::from(2000));
    }

    proptest! {
        #[test]
        fn handshake_and_cayley(n in 2usize..12, mask in any::<u64>()) {
            // Σ deg = 2|E| for any graph; K_n has n^{n−2} spanning trees.
            let all: Vec<(usize, usize)> = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).collect();
            let edges: Vec<(usize, usize)> = all.iter().enumerate().filter(|(i, _)| mask >> (i % 64) & 1 == 1).map(|(_, &e)| e).collect();
            let g = Graph::from_edges(n, &edges).unwrap();
            prop_assert_eq!(g.degrees().iter().sum::<usize>(), 2 * edges.len());
            let k = Graph::complete(n).unwrap();
            prop_assert_eq!(k.spanning_trees(), BigInt::from(n).pow(n as u32 - 2));
            // A tree has exactly one spanning tree.
            if g.is_tree() { prop_assert_eq!(g.spanning_trees(), BigInt::from(1)); }
        }
    }
}
