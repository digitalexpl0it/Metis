//! Shared GTK callback type aliases.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::Button;
use gtk::Image;

pub type Fn0 = Rc<dyn Fn()>;
pub type OptFn0Cell = Rc<RefCell<Option<Fn0>>>;
pub type ClipboardItemCells = Rc<RefCell<Vec<(Button, Image, usize)>>>;
