/*
* @Author: BlahGeek
* @Date:   2017-04-23
* @Last Modified by:   BlahGeek
* @Last Modified time: 2017-07-15
*/

extern crate glib;
extern crate libc;
extern crate glib_sys;

use toml;

use std;
use std::ffi;
use std::error::Error;
use frontend_gtk::gdk;
use frontend_gtk::gtk;
use frontend_gtk::gtk::prelude::*;

use std::thread;
use std::sync::mpsc;
use std::cell::RefCell;
use std::rc::Rc;

use frontend_gtk::ui::MinionsUI;
use mcore::context::Context;
use mcore::action::ActionResult;
use mcore::item::Item;

const FILTER_TEXT_CLEAR_TIME: u32 = 1;

#[derive(Clone)]
enum Status {
    Initial,
    Running(Rc<mpsc::Receiver<ActionResult>>),
    Error(Rc<Box<Error>>), // Rc is for Clone
    FilteringNone,
    FilteringEntering {
        selected_idx: i32,
        filter_text: String,
        filter_text_lasttime: std::time::Instant,
        filter_indices: Vec<usize>,
    },
    FilteringMoving {
        selected_idx: i32,
        filter_text: String,
        filter_indices: Vec<usize>,
    },
    EnteringText(usize), // entering text for item, (index for list_items)
}

pub struct MinionsApp {
    ui: MinionsUI,
    ctx: Context,

    status: Status,
}


thread_local! {
    static APP: RefCell<Option<MinionsApp>> = RefCell::new(None);
}


#[link(name="keybinder-3.0")]
extern {
    fn keybinder_init();
    fn keybinder_bind(keystring: *const libc::c_char,
                      handler: extern fn(*const libc::c_char, *mut libc::c_void),
                      user_data: *mut libc::c_void) -> glib_sys::gboolean;
}

extern fn keybinder_callback_show(_: *const libc::c_char, _: *mut libc::c_void) {
    info!("keybinder callback: show");
    glib::idle_add( move || {
        APP.with(|app| {
            if let Some(ref mut app) = *app.borrow_mut() {
                app.reset_window(false);
            }
        });
        Continue(false)
    });
}

extern fn keybinder_callback_show_clipboard(_: *const libc::c_char, _: *mut libc::c_void) {
    info!("keybinder callback: show with clipboard");
    glib::idle_add( move || {
        APP.with(|app| {
            if let Some(ref mut app) = *app.borrow_mut() {
                app.reset_window(true);
            }
        });
        Continue(false)
    });
}

impl MinionsApp {

    fn update_ui(&self) {
        trace!("update ui");
        match self.status {
            Status::Initial => {
                self.ui.set_entry(None);
                self.ui.set_filter_text("");
                self.ui.set_action_name(None);
                self.ui.set_reference(None);
                self.ui.set_items(Vec::new(), -1, &self.ctx);
                self.ui.set_spinning(false);
            },
            Status::Running(_) => {
                self.ui.set_entry(None);
                self.ui.set_filter_text("");
                self.ui.set_action_name(None);
                self.ui.set_reference(None);
                self.ui.set_items(Vec::new(), -1, &self.ctx);
                self.ui.set_spinning(true);
            },
            Status::Error(ref error) => {
                self.ui.set_entry(None);
                self.ui.set_filter_text("");
                self.ui.set_action_name(None);
                self.ui.set_reference(None);
                self.ui.set_items(Vec::new(), -1, &self.ctx);
                self.ui.set_spinning(false);
                self.ui.set_error(&error);
            },
            Status::FilteringNone => {
                self.ui.set_spinning(false);
                self.ui.set_entry(None);
                self.ui.set_filter_text("");
                self.ui.set_action_name(None);
                self.ui.set_reference(self.ctx.reference.as_ref());
                if self.ctx.list_items.len() == 0 {
                    warn!("No more listing items!");
                    self.ui.window.hide();
                }
                self.ui.set_items(self.ctx.list_items.iter().collect(), -1, &self.ctx);
            },
            Status::FilteringEntering {
                selected_idx,
                ref filter_text,
                filter_text_lasttime: _,
                ref filter_indices
            } |
            Status::FilteringMoving {
                selected_idx,
                ref filter_text,
                ref filter_indices
            } => {
                if selected_idx < 0 {
                    self.ui.set_entry(None);
                } else {
                    self.ui.set_entry(Some(&self.ctx.list_items[filter_indices[selected_idx as usize]]))
                }
                self.ui.set_spinning(false);
                self.ui.set_filter_text(&filter_text);
                self.ui.set_action_name(None);
                self.ui.set_reference(self.ctx.reference.as_ref());
                self.ui.set_items(filter_indices.iter().map(|x| &self.ctx.list_items[x.clone()])
                                  .collect::<Vec<&Item>>(), selected_idx, &self.ctx);
            },
            Status::EnteringText(idx) => {
                self.ui.set_spinning(false);
                self.ui.set_entry(None);
                self.ui.set_entry_editable();
                self.ui.set_filter_text("");
                // self.ui.set_reference_item(Some(&self.ctx.list_items[idx]));
                self.ui.set_action_name(Some(&self.ctx.list_items[idx].title));
                self.ui.set_reference(None);
                self.ui.set_items(Vec::new(), -1, &self.ctx);
            }
        }
    }

    fn process_timeout(&mut self) {
        if let Status::FilteringEntering {
            selected_idx: _,
            filter_text: _,
            filter_text_lasttime,
            filter_indices: _
        } = self.status {
            if filter_text_lasttime.elapsed() >= std::time::Duration::new(FILTER_TEXT_CLEAR_TIME as u64, 0) {
                self.status = Status::FilteringNone;
                self.update_ui();
            }
        }
    }

    fn process_keyevent_escape(&mut self) {
        trace!("Processing keyevent Escape");
        self.status = match self.status {
            Status::Initial => {
                debug!("Quit!");
                self.ui.window.hide();
                Status::Initial
            },
            Status::FilteringNone => {
                self.ctx.reset();
                Status::Initial
            },
            Status::Running(_) => {
                warn!("Drop thread");
                Status::FilteringNone
            }
            _ => Status::FilteringNone,
        };
        self.update_ui();
    }

    fn process_keyevent_move(&mut self, delta: i32) {
        trace!("Processing keyevent Move: {}", delta);
        self.status = match self.status.clone() {
            Status::FilteringNone | Status::Initial => {
                Status::FilteringMoving {
                    selected_idx: 0,
                    filter_text: String::new(),
                    filter_indices: (0..self.ctx.list_items.len()).collect(),
                }
            },
            Status::FilteringEntering {
                selected_idx,
                filter_text,
                filter_text_lasttime: _,
                filter_indices
            } |
            Status::FilteringMoving {
                selected_idx,
                filter_text,
                filter_indices
            } => {
                let mut new_idx = selected_idx + delta;
                if filter_indices.len() == 0 {
                    new_idx = -1;
                } else {
                    if new_idx >= filter_indices.len() as i32 {
                        new_idx = filter_indices.len() as i32 - 1;
                    }
                    if new_idx < 0 {
                        new_idx = 0;
                    }
                }
                Status::FilteringMoving {
                    selected_idx: new_idx,
                    filter_text: filter_text,
                    filter_indices: filter_indices
                }
            },
            status @ _ => status,
        };
        self.update_ui();
    }

    fn process_keyevent_tab(&mut self) {
        trace!("Processing keyevent Tab");
        self.status = match self.status.clone() {
            Status::FilteringEntering {
                selected_idx,
                filter_text: _,
                filter_text_lasttime: _,
                filter_indices
            } |
            Status::FilteringMoving {
                selected_idx,
                filter_text: _,
                filter_indices
            } => {
                if selected_idx < 0 {
                    warn!("No item to send");
                    self.status.clone()
                } else {
                    let item = self.ctx.list_items[filter_indices[selected_idx as usize]].clone();
                    if self.ctx.quicksend_able(&item) {
                        if let Err(error) = self.ctx.quicksend(item) {
                            warn!("Unable to quicksend item: {}", error);
                            Status::Error(Rc::new(error))
                        } else {
                            Status::FilteringNone
                        }
                    } else {
                        warn!("Item not sendable");
                        self.status.clone()
                    }
                }
            },
            status @ _ => status,
        };
        self.update_ui();
    }

    fn _make_status_filteringentering(&self, text: String) -> Status {
        let filter_indices = self.ctx.filter(&text);
        let selected_idx = if filter_indices.len() == 0 { -1 } else { 0 };

        gtk::timeout_add_seconds(FILTER_TEXT_CLEAR_TIME, move || {
            APP.with(|app| {
                if let Some(ref mut app) = *app.borrow_mut() {
                    app.process_timeout();
                }
                Continue(false)
            })
        });

        Status::FilteringEntering {
            selected_idx: selected_idx,
            filter_text: text,
            filter_text_lasttime: std::time::Instant::now(),
            filter_indices: filter_indices,
        }
    }

    fn process_keyevent_char(&mut self, ch: char) {
        trace!("Processing keyevent Char: {}", ch);
        let mut should_update_ui = true;
        self.status = match self.status.clone() {
            Status::Initial | Status::FilteringNone => {
                let mut text = String::new();
                text.push(ch);
                self._make_status_filteringentering(text)
            },
            Status::FilteringEntering {
                selected_idx: _,
                mut filter_text,
                filter_text_lasttime: _,
                filter_indices: _
            } |
            Status::FilteringMoving {
                selected_idx: _,
                mut filter_text,
                filter_indices: _,
            } => {
                filter_text.push(ch);
                self._make_status_filteringentering(filter_text)
            },
            status @ _ => {
                should_update_ui = false;
                status
            },
        };
        if should_update_ui {
            self.update_ui();
        }
    }

    fn process_keyevent_space(&mut self) {
        trace!("Processing keyevent Space");
        let mut should_update_ui = false;
        self.status = match self.status.clone() {
            Status::FilteringEntering {
                selected_idx,
                filter_text: _,
                filter_text_lasttime: _,
                filter_indices
            } |
            Status::FilteringMoving {
                selected_idx,
                filter_text: _,
                filter_indices
            } => {
                if selected_idx < 0 {
                    warn!("No item to select");
                    self.status.clone()
                } else {
                    let idx = filter_indices[selected_idx as usize];
                    let item = &self.ctx.list_items[idx];
                    if self.ctx.selectable_with_text(item) {
                        should_update_ui = true;
                        Status::EnteringText(idx)
                    } else {
                        warn!("Item not selectable with or without text");
                        self.status.clone()
                    }
                }
            },
            status @ _ => status,
        };

        if should_update_ui {
            self.update_ui();
        }
    }

    fn process_running_callback(&mut self) {
        let mut res : Option<ActionResult> = None;
        if let Status::Running(ref recv_ch) = self.status {
            if let Ok(res_) = recv_ch.try_recv() {
                trace!("Received result on callback");
                res = Some(res_);
            } else {
                warn!("Unable to receive from channel");
            }
        }

        if let Some(res) = res {
            self.status = match res {
                Ok(res) => {
                    self.ctx.async_select_callback(res);
                    Status::FilteringNone
                },
                Err(error) => {
                    warn!("Error from channel: {}", error);
                    Status::Error(Rc::new(error))
                }
            };
            self.update_ui();
        } else {
            warn!("No action result");
        }
    }

    fn process_keyevent_enter(&mut self) {
        trace!("Processing keyevent Enter");
        self.status = match self.status.clone() {
            status @ Status::Initial | status @ Status::FilteringNone => status,
            Status::FilteringEntering {
                selected_idx,
                filter_text: _,
                filter_text_lasttime: _,
                filter_indices
            } |
            Status::FilteringMoving {
                selected_idx,
                filter_text: _,
                filter_indices
            } => {
                if selected_idx < 0 {
                    warn!("No item to select");
                    self.status.clone()
                } else {
                    let idx = filter_indices[selected_idx as usize];
                    let item = self.ctx.list_items[idx].clone();

                    let (send_ch, recv_ch) = mpsc::channel::<ActionResult>();
                    if self.ctx.selectable(&item) {
                        self.ctx.async_select(item, move |res: ActionResult| {
                            if let Err(error) = send_ch.send(res) {
                                warn!("Unable to send to channel: {}", error);
                            } else {
                                glib::idle_add( || {
                                    APP.with(move |app| app.borrow_mut().as_mut().unwrap().process_running_callback() );
                                    Continue(false)
                                });
                            }
                        });
                        Status::Running(Rc::new(recv_ch))
                    } else if self.ctx.selectable_with_text(&item) {
                        Status::EnteringText(idx)
                    } else {
                        warn!("Item not selectable with or without text");
                        self.status.clone()
                    }
                }
            },
            Status::EnteringText(idx) => {
                let text = self.ui.get_entry_text();
                let item = self.ctx.list_items[idx].clone();
                let (send_ch, recv_ch) = mpsc::channel::<ActionResult>();

                self.ctx.async_select_with_text(item, &text, move |res: ActionResult| {
                    if let Err(error) = send_ch.send(res) {
                        warn!("Unable to send to channel: {}", error);
                    } else {
                        glib::idle_add( || {
                            APP.with(move |app| app.borrow_mut().as_mut().unwrap().process_running_callback() );
                            Continue(false)
                        });
                    }
                });
                Status::Running(Rc::new(recv_ch))
            },
            status @ _ => status,
        };
        self.update_ui();
    }

    fn process_keyevent_copy(&mut self) {
        trace!("Process keyevent copy");
        self.status = match self.status.clone() {
            Status::FilteringEntering {
                selected_idx,
                filter_text,
                filter_text_lasttime: _,
                filter_indices,
            } |
            Status::FilteringMoving {
                selected_idx,
                filter_text,
                filter_indices,
            } => {
                let idx = filter_indices[selected_idx as usize];
                if let Err(error) = self.ctx.copy_content_to_clipboard(
                                            &self.ctx.list_items[idx]) {
                    warn!("Unable to copy item: {}", error);
                } else {
                    info!("Item copied");
                }
                Status::FilteringMoving{selected_idx, filter_text, filter_indices}
            },
            status @ _ => { status },
        };
    }

    fn process_keyevent(&mut self, event: &gdk::EventKey) -> Inhibit {
        let key = event.get_keyval();
        let modi = event.get_state();
        trace!("Key pressed: {:?}/{:?}", key, modi);
        if key == gdk::enums::key::Return {
            self.process_keyevent_enter();
            Inhibit(true)
        } else if key == gdk::enums::key::space {
            self.process_keyevent_space();
            Inhibit(false)
        } else if key == gdk::enums::key::Escape {
            self.process_keyevent_escape();
            Inhibit(true)
        } else if key == gdk::enums::key::Tab {
            self.process_keyevent_tab();
            Inhibit(true)
        } else if key == 'j' as u32 && modi == gdk::CONTROL_MASK {
            self.process_keyevent_move(1);
            Inhibit(true)
        } else if key == 'k' as u32 && modi == gdk::CONTROL_MASK {
            self.process_keyevent_move(-1);
            Inhibit(true)
        } else if key == 'c' as u32 && modi == gdk::CONTROL_MASK {
            self.process_keyevent_copy();
            Inhibit(true)
        } else if key == gdk::enums::key::Down {
            self.process_keyevent_move(1);
            Inhibit(true)
        } else if key == gdk::enums::key::Up {
            self.process_keyevent_move(-1);
            Inhibit(true)
        } else if let Some(ch) = gdk::keyval_to_unicode(key) {
            if ch.is_alphabetic() {
                self.process_keyevent_char(ch);
            } else {
                trace!("Ignore char: {}", ch);
            }
            Inhibit(false)
        } else {
            Inhibit(false)
        }
    }

    fn reset_window(&mut self, send_clipboard: bool) {
        trace!("Resetting window: {}", send_clipboard);
        self.ctx.reset();
        self.status = Status::Initial;
        if send_clipboard {
            if let Err(error) = self.ctx.quicksend_from_clipboard() {
                warn!("Unable to get content from clipboard: {}", error);
            } else {
                self.status = Status::FilteringNone;
            }
        }
        self.ui.window.show();
        self.update_ui();
    }

    pub fn new(config: toml::Value) -> &'static thread::LocalKey<RefCell<Option<MinionsApp>>> {
        let app = MinionsApp {
            ui: MinionsUI::new(),
            ctx: Context::new(config),
            status: Status::Initial,
        };
        app.update_ui();
        app.ui.window.hide();

        app.ui.window.connect_key_press_event(move |_, event| {
            APP.with(|app| {
                if let Some(ref mut app) = *app.borrow_mut() {
                    app.process_keyevent(event)
                } else { Inhibit(false) }
            })
        });

        unsafe {
            keybinder_init();
            {
                let s = ffi::CString::new("<Ctrl>space").unwrap();
                keybinder_bind(s.as_ptr(), keybinder_callback_show, std::ptr::null_mut());
            }
            {
                let s = ffi::CString::new("<Ctrl><Shift>space").unwrap();
                keybinder_bind(s.as_ptr(), keybinder_callback_show_clipboard, std::ptr::null_mut());
            }
        }

        app.ui.window.connect_delete_event(move |_, _| {
            gtk::main_quit();
            Inhibit(false)
        });

        APP.with(|g_app| *g_app.borrow_mut() = Some(app) );
        &APP
    }
}
