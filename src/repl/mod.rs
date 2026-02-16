use crate::error::Result;
use crate::ui;

pub fn run() -> Result<()> {
    let app = ui::app::App::new()?;
    let mut terminal = ui::terminal::init()?;
    let result = app.run(&mut terminal);
    ui::terminal::restore()?;
    result
}
