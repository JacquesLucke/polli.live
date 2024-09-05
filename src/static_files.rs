use include_dir::include_dir;

static STATIC_FILES: include_dir::Dir = include_dir!("static");

pub fn get(filename: &str) -> &'static str {
    let file = STATIC_FILES.get_file(filename).unwrap();
    file.contents_utf8().unwrap()
}
