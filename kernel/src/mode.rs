#[derive(Default, PartialEq, Clone, Copy)]
pub enum Mode {
    #[default]
    Navigation,
    Search,
    SearchResults,
}

