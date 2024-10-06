//! the table for displaying the repo and repo list

use std::fmt;

use comfy_table::presets::UTF8_FULL;
use comfy_table::TableComponent::{
    BottomLeftCorner, BottomRightCorner, TopLeftCorner, TopRightCorner,
};
use comfy_table::{Attribute, Cell, Color};

use crate::storage::Repo;

static DISPLAY_HEADER: [&str; 3] = ["Name", "Url", "Version"];

pub struct Table(pub comfy_table::Table);

impl Default for Table {
    fn default() -> Self {
        let mut table = comfy_table::Table::new();
        table.load_preset(UTF8_FULL);
        table.set_style(TopLeftCorner, '╭');
        table.set_style(TopRightCorner, '╮');
        table.set_style(BottomLeftCorner, '╰');
        table.set_style(BottomRightCorner, '╯');
        table.set_header(DISPLAY_HEADER);
        Self(table)
    }
}

impl Table {
    pub fn add(mut self, repo: &Repo) -> Self {
        self.add_row(repo);
        self
    }

    pub fn add_row(&mut self, repo: &Repo) {
        self.0.add_row(vec![
            Cell::new(repo.name.clone())
                .add_attribute(Attribute::Bold)
                .fg(Color::Green),
            Cell::new(repo.url().to_string()),
            Cell::new(repo.version.clone().unwrap_or_default()),
        ]);
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::Repo;

    #[test]
    #[ignore = "manual run to see the output"]
    fn test_print_repo() {
        println!(
            "{}",
            Repo::new("bpm-rs").set_by_url("https://github.com/lxl66566/bpm-rs")
        );
    }
}
