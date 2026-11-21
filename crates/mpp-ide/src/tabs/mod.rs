//! Tabs: back/forward history and breadcrumbs (the tab strips themselves are
//! tiles, see `layout`).

pub mod breadcrumbs;
pub mod nav;

/// Where a dragged tab ends up when `from` is dropped on `to`: the new order
/// of indices and the new active index.
pub fn reorder(len: usize, from: usize, to: usize, active: usize) -> (Vec<usize>, usize) {
    let mut order: Vec<usize> = (0..len).collect();
    if from >= len || to >= len || from == to {
        return (order, active);
    }
    let item = order.remove(from);
    order.insert(to, item);
    let new_active = order.iter().position(|&o| o == active).unwrap_or(0);
    (order, new_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_a_tab_keeps_the_active_one_active() {
        let (order, active) = reorder(4, 0, 2, 0);
        assert_eq!(order, vec![1, 2, 0, 3]);
        assert_eq!(active, 2);
        let (order, active) = reorder(3, 2, 0, 1);
        assert_eq!(order, vec![2, 0, 1]);
        assert_eq!(active, 2);
        assert_eq!(reorder(3, 5, 0, 1).0, vec![0, 1, 2]);
    }
}
