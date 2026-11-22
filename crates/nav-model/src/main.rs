fn main() {
    let report = nav_model::check();
    print!("{}", report.render());
    if !report.ok() {
        std::process::exit(1);
    }
}
