//! A segmented switch of mutually exclusive options.
//!
//! AdwToggleGroup only exists in libadwaita 1.7 and newer. This app targets 1.5 so
//! it runs on Ubuntu 24.04, Fedora 40, Debian 13 and anything newer, so the widget
//! is reimplemented here on top of plain toggles: same look through the `.tm-modes`
//! CSS class, same `active_name` semantics.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

type Callback = Rc<dyn Fn(&ToggleGroup)>;

#[derive(Clone)]
pub struct Toggle {
    button: gtk::ToggleButton,
    name: String,
}

impl Toggle {
    pub fn new() -> Self {
        let button = gtk::ToggleButton::new();
        button.add_css_class("tm-toggle");
        button.set_valign(gtk::Align::Center);
        Self { button, name: String::new() }
    }

    pub fn set_name(&self, name: Option<&str>) {
        // Name has to be mutable after construction: address it through the widget
        // name, which GTK keeps for us.
        if let Some(n) = name {
            self.button.set_widget_name(n);
        }
    }

    pub fn set_label(&self, label: Option<&str>) {
        if let Some(text) = label {
            self.button.set_label(text);
        }
    }

    pub fn set_tooltip(&self, text: &str) {
        self.button.set_tooltip_text(Some(text));
    }
}

impl Default for Toggle {
    fn default() -> Self {
        Self::new()
    }
}

struct Inner {
    toggles: RefCell<Vec<gtk::ToggleButton>>,
    callbacks: RefCell<Vec<Callback>>,
    active: RefCell<Option<String>>,
    quiet: RefCell<bool>,
}

#[derive(Clone)]
pub struct ToggleGroup {
    /// The widget to put in the interface.
    pub root: gtk::Box,
    inner: Rc<Inner>,
}

impl ToggleGroup {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("linked");
        root.set_valign(gtk::Align::Center);
        Self {
            root,
            inner: Rc::new(Inner {
                toggles: RefCell::new(Vec::new()),
                callbacks: RefCell::new(Vec::new()),
                active: RefCell::new(None),
                quiet: RefCell::new(false),
            }),
        }
    }

    pub fn add_css_class(&self, class: &str) {
        self.root.add_css_class(class);
    }

    pub fn set_homogeneous(&self, homogeneous: bool) {
        self.root.set_homogeneous(homogeneous);
    }

    /// Kept for API parity: the plain box always shrinks to its labels.
    pub fn set_can_shrink(&self, _can_shrink: bool) {}

    pub fn add(&self, toggle: Toggle) {
        let button = toggle.button;
        let name = if toggle.name.is_empty() {
            button.widget_name().to_string()
        } else {
            toggle.name
        };
        button.set_widget_name(&name);
        self.inner.toggles.borrow_mut().push(button.clone());
        self.root.append(&button);

        // One toggle on means every other one off.
        let inner = self.inner.clone();
        let this = self.clone();
        let own_name = name.clone();
        button.connect_toggled(move |b| {
            if *inner.quiet.borrow() {
                return;
            }
            if b.is_active() {
                *inner.quiet.borrow_mut() = true;
                for other in inner.toggles.borrow().iter() {
                    if other.widget_name() != own_name {
                        other.set_active(false);
                    }
                }
                *inner.quiet.borrow_mut() = false;
                *inner.active.borrow_mut() = Some(own_name.clone());
                for cb in inner.callbacks.borrow().iter() {
                    cb(&this);
                }
            } else if inner.active.borrow().as_deref() == Some(own_name.as_str()) {
                // Clicking the active toggle keeps it active: a segmented control
                // always has a selection.
                b.set_active(true);
            }
        });
    }

    pub fn set_active_name(&self, name: Option<&str>) {
        let Some(name) = name else { return };
        *self.inner.quiet.borrow_mut() = true;
        for b in self.inner.toggles.borrow().iter() {
            b.set_active(b.widget_name() == name);
        }
        *self.inner.quiet.borrow_mut() = false;
        let changed = self.inner.active.borrow().as_deref() != Some(name);
        *self.inner.active.borrow_mut() = Some(name.to_string());
        if changed {
            for cb in self.inner.callbacks.borrow().iter() {
                cb(self);
            }
        }
    }

    pub fn active_name(&self) -> Option<String> {
        self.inner.active.borrow().clone()
    }

    pub fn connect_active_name_notify<F: Fn(&ToggleGroup) + 'static>(&self, f: F) {
        self.inner.callbacks.borrow_mut().push(Rc::new(f));
    }
}

impl Default for ToggleGroup {
    fn default() -> Self {
        Self::new()
    }
}
