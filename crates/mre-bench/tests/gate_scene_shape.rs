//! Gate: the scene's *shape* must not change with the data set.
//!
//! No timing and no allocation counting here, so this is safe on a noisy CI
//! runner. It catches the failure mode where virtualisation quietly stops
//! virtualising: the table still looks right, but a 50k-row data set now
//! materialises 50k nodes.

use mre_scene::ROW_HEIGHT;

#[test]
fn scrolling_never_grows_the_scene() {
    let (mut view, mut list) = mre_bench::warm_view(30, 0.0);
    let nodes = view.node_count();
    let bytes = view.scene_bytes();

    for _ in 0..5_000 {
        view.frame(-ROW_HEIGHT, &mut list);
    }

    assert_eq!(
        view.node_count(),
        nodes,
        "node count changed while scrolling"
    );
    assert_eq!(view.scene_bytes(), bytes, "arena grew while scrolling");
}

#[test]
fn node_count_is_independent_of_row_count() {
    let small = mre_scene::TableView::new(1_000, mre_bench::viewport());
    let large = mre_scene::TableView::new(10_000_000, mre_bench::viewport());

    assert_eq!(
        small.node_count(),
        large.node_count(),
        "a 10-million-row table must not allocate more nodes than a 1-thousand-row one"
    );
}

#[test]
fn display_list_never_reallocates_during_frames() {
    let mut list = mre_scene::DisplayList::with_capacity(mre_bench::DISPLAY_LIST_CAPACITY);
    let capacity = list.capacity();
    let (mut view, _) = mre_bench::warm_view(10, 0.0);

    for _ in 0..300 {
        view.frame(4.0, &mut list);
    }

    assert_eq!(
        list.capacity(),
        capacity,
        "display list reallocated during frames"
    );
    assert!(
        list.len() <= capacity,
        "display list overflowed its reserved capacity"
    );
}
