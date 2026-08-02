use strukt_terminal::{CellWidth, EraseDisplay, EraseLine, Grid, GridSize};

#[test]
fn wide_cells_wrap_without_orphan_continuations() {
    let mut grid = Grid::new(GridSize::new(2, 4).unwrap(), 10);
    grid.print("abc界");

    let snapshot = grid.snapshot(0);
    assert_eq!(snapshot.plain_text(), "abc \n界  ");
    assert!(
        snapshot
            .rows()
            .iter()
            .flatten()
            .all(strukt_terminal::Cell::is_structurally_valid)
    );
}

#[test]
fn scrollback_is_bounded_and_alternate_screen_does_not_pollute_it() {
    let mut grid = Grid::new(GridSize::new(2, 3).unwrap(), 2);
    grid.print("1\r\n2\r\n3\r\n4");
    assert_eq!(grid.scrollback_len(), 2);

    grid.enter_alternate_screen();
    grid.print("alt");
    grid.leave_alternate_screen();

    assert_eq!(grid.scrollback_len(), 2);
}

#[test]
fn resize_clamps_the_cursor_and_repairs_wide_cell_boundaries() {
    let mut grid = Grid::new(GridSize::new(2, 4).unwrap(), 10);
    grid.print("ab界");
    grid.set_cursor(99, 99);

    let outcome = grid.resize(GridSize::new(2, 3).unwrap());
    let snapshot = grid.snapshot(0);

    assert!(outcome.changed());
    assert_eq!(snapshot.cursor().row, 1);
    assert_eq!(snapshot.cursor().column, 2);
    for row in snapshot.rows() {
        for (column, cell) in row.iter().enumerate() {
            match cell.width() {
                CellWidth::Wide => {
                    assert_eq!(
                        row.get(column + 1).unwrap().width(),
                        CellWidth::Continuation
                    );
                }
                CellWidth::Continuation => {
                    assert!(column > 0);
                    assert_eq!(row[column - 1].width(), CellWidth::Wide);
                }
                CellWidth::Single => {}
            }
        }
    }
}

#[test]
fn resize_reflows_a_wide_cell_that_no_longer_fits_the_row() {
    let mut grid = Grid::new(GridSize::new(2, 4).unwrap(), 10);
    grid.print("ab界");

    grid.resize(GridSize::new(2, 3).unwrap());

    assert_eq!(grid.snapshot(0).plain_text(), "ab \n界 ");
}

#[test]
fn erasure_and_character_editing_preserve_row_width() {
    let mut grid = Grid::new(GridSize::new(2, 6).unwrap(), 10);
    grid.print("abcdef");
    grid.set_cursor(0, 2);
    grid.delete_characters(2);
    assert_eq!(grid.snapshot(0).plain_text(), "abef  \n      ");

    grid.insert_blank_characters(1);
    assert_eq!(grid.snapshot(0).plain_text(), "ab ef \n      ");

    grid.erase_in_line(EraseLine::All);
    assert_eq!(grid.snapshot(0).plain_text(), "      \n      ");

    grid.print("x");
    grid.erase_in_display(EraseDisplay::All);
    assert_eq!(grid.snapshot(0).plain_text(), "      \n      ");
}

#[test]
fn scroll_regions_and_reverse_index_do_not_move_outside_rows() {
    let mut grid = Grid::new(GridSize::new(4, 3).unwrap(), 10);
    grid.print("top\r\n111\r\n222\r\nbot");
    grid.set_scroll_region(1, 2).unwrap();
    grid.set_cursor(2, 0);
    grid.scroll_up(1);
    assert_eq!(grid.snapshot(0).plain_text(), "top\n222\n   \nbot");

    grid.set_cursor(1, 0);
    grid.reverse_index();
    assert_eq!(grid.snapshot(0).plain_text(), "top\n   \n222\nbot");
}

#[test]
fn line_editing_and_viewport_offsets_are_bounded() {
    let mut grid = Grid::new(GridSize::new(3, 3).unwrap(), 4);
    grid.print("111\r\n222\r\n333");
    grid.set_cursor(1, 0);
    grid.insert_blank_lines(1);
    assert_eq!(grid.snapshot(0).plain_text(), "111\n   \n222");
    grid.delete_lines(1);
    assert_eq!(grid.snapshot(0).plain_text(), "111\n222\n   ");

    grid.set_cursor(2, 0);
    grid.print("444\r\n555");
    let snapshot = grid.snapshot(usize::MAX);
    assert_eq!(snapshot.viewport_offset(), grid.scrollback_len());
    assert_eq!(snapshot.rows().len(), 3);
}
