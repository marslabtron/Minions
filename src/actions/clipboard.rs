/*
* @Author: BlahGeek
* @Date:   2017-07-16
* @Last Modified by:   BlahGeek
* @Last Modified time: 2017-07-16
*/

extern crate gtk;
extern crate glib;
extern crate gdk;
extern crate gtk_sys;
extern crate libc;
extern crate chrono;

use self::glib::signal::connect;
use self::glib::translate::*;
use self::gtk::Clipboard;
use self::chrono::{Local, DateTime, TimeZone};

use std::sync::{Arc, Mutex};
use std::mem::transmute;
use std::collections::VecDeque;
use std::time::SystemTime;

use actions::ActionError;
use mcore::action::{Action, ActionResult};
use mcore::item::{Item, Icon};

unsafe extern "C" fn trampoline(clipboard: *mut gtk_sys::GtkClipboard,
                                _: *mut libc::c_void,
                                f: &Box<Fn(&Clipboard) + 'static>) {
    f(&Clipboard::from_glib_none(clipboard))
}


fn connect_clipboard_change<F>(clipboard: &Clipboard, f: F)
where F: Fn(&Clipboard) + 'static {
    unsafe {
        let f: Box<Box<Fn(&Clipboard) + 'static>> =
            Box::new(Box::new(f));
        connect(clipboard.to_glib_none().0, "owner-change",
                transmute(trampoline as usize), Box::into_raw(f) as *mut _);
    }
}


pub struct ClipboardHistoryAction {
    history_max_len: usize,
    history: Arc<Mutex<VecDeque<(String, DateTime<Local>)>>>,
}

impl Action for ClipboardHistoryAction {
    fn get_item(&self) -> Item {
        let mut item = Item::new("Clipboard History");
        item.subtitle = Some(format!("View clipboard history up to {} entries", self.history_max_len));
        item.icon = Some(Icon::Character{ch: '', font: "FontAwesome".into()});
        item
    }

    fn accept_nothing(&self) -> bool { true }

    fn run(&self) -> ActionResult {
        if let Ok(history) = self.history.lock() {
            debug!("Returning {} clipboard histories", history.len());
            if history.len() == 0 {
                Err(Box::new(ActionError::new("No clipboard history available")))
            } else {
                Ok(history.iter().map(|x| {
                    let mut item = Item::new_text_item(&x.0);
                    item.subtitle = Some(format!("{}, {} bytes",
                                                 x.1.format("%T %b %e").to_string(),
                                                 x.0.len()));
                    item.icon = Some(Icon::Character{ch: '', font: "FontAwesome".into()});
                    item
                }).collect())
            }
        } else {
            Err(Box::new(ActionError::new("Unable to unlock history")))
        }
    }
}

impl ClipboardHistoryAction {
    pub fn new(history_max_len: usize) -> ClipboardHistoryAction {
        let action = ClipboardHistoryAction {
            history_max_len: history_max_len,
            history: Arc::new(Mutex::new(VecDeque::new())),
        };
        let history = action.history.clone();

        let clipboard = gtk::Clipboard::get(&gdk::Atom::intern("CLIPBOARD"));
        connect_clipboard_change(&clipboard, move |clipboard| {
            let content = clipboard.wait_for_text();
            if let Some(text) = content {
                trace!("New clipboard text: {:?}", text);
                if let Ok(mut history) = history.lock() {
                    let is_dup = if let Some(front) = history.front() {
                        text == front.0.as_str()
                    } else {
                        false
                    };
                    if is_dup {
                        debug!("Duplicate, do not push to history");
                    } else {
                        history.push_front((text.into(), Local::now()));
                    }
                    while history.len() > history_max_len {
                        history.pop_back().unwrap();
                    }
                }
            }
        });
        action
    }
}

