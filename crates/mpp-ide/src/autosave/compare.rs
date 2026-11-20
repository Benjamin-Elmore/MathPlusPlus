//! A side-by-side compare of two versions of a file, block by block, as typeset
//! LaTeX (never the raw lines).

use mpp_syntax::document::parse_document;

use crate::explorer::search::block_latex;

/// How a row differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Same,
    Changed,
    Added,
    Removed,
}

/// One row of the compare: what each side shows (`None` for a gap).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub left: Option<String>,
    pub right: Option<String>,
    pub kind: RowKind,
}

/// The most blocks compared per side.
const MAX_BLOCKS: usize = 300;

struct Item {
    key: String,
    latex: String,
}

fn items(src: &str) -> Vec<Item> {
    let doc = parse_document(src);
    let mut v = vec![];
    doc.walk(&mut |b| {
        if v.len() < MAX_BLOCKS {
            if let Some(t) = src.get(b.span.start..b.span.end) {
                v.push(Item {
                    key: t.trim().to_string(),
                    latex: block_latex(b, src),
                });
            }
        }
    });
    v
}

/// Compares two texts.
pub fn compare(left: &str, right: &str) -> Vec<Row> {
    let (a, b) = (items(left), items(right));
    let (n, m) = (a.len(), b.len());
    // Longest common subsequence of equal blocks.
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i].key == b[j].key {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut rows = vec![];
    let (mut i, mut j) = (0, 0);
    let (mut gone, mut new): (Vec<&Item>, Vec<&Item>) = (vec![], vec![]);
    let flush = |rows: &mut Vec<Row>, gone: &mut Vec<&Item>, new: &mut Vec<&Item>| {
        let pairs = gone.len().max(new.len());
        for k in 0..pairs {
            let (l, r) = (gone.get(k), new.get(k));
            let kind = match (l, r) {
                (Some(_), Some(_)) => RowKind::Changed,
                (Some(_), None) => RowKind::Removed,
                _ => RowKind::Added,
            };
            rows.push(Row {
                left: l.map(|x| x.latex.clone()),
                right: r.map(|x| x.latex.clone()),
                kind,
            });
        }
        gone.clear();
        new.clear();
    };
    while i < n || j < m {
        if i < n && j < m && a[i].key == b[j].key {
            flush(&mut rows, &mut gone, &mut new);
            rows.push(Row {
                left: Some(a[i].latex.clone()),
                right: Some(b[j].latex.clone()),
                kind: RowKind::Same,
            });
            i += 1;
            j += 1;
        } else if j >= m || (i < n && lcs[i + 1][j] >= lcs[i][j + 1]) {
            gone.push(&a[i]);
            i += 1;
        } else {
            new.push(&b[j]);
            j += 1;
        }
    }
    flush(&mut rows, &mut gone, &mut new);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_changed_added_and_removed_blocks_are_told_apart() {
        let a = "# Title\n\n$$ x = 1 $$\n\n$$ y = 2 $$\n";
        let b = "# Title\n\n$$ x = 5 $$\n\n$$ y = 2 $$\n\n$$ z = 3 $$\n";
        let rows = compare(a, b);
        let kinds: Vec<RowKind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Same,
                RowKind::Changed,
                RowKind::Same,
                RowKind::Added
            ]
        );
        assert!(rows[1].left.as_deref().is_some_and(|l| l.contains('1')));
        assert!(rows[1].right.as_deref().is_some_and(|l| l.contains('5')));
        assert!(compare(a, a).iter().all(|r| r.kind == RowKind::Same));
        assert_eq!(
            compare(a, "").last().map(|r| r.kind),
            Some(RowKind::Removed)
        );
    }
}
