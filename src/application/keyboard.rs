use cascade::cascade;
use glib::object::WeakRef;
use glib::subclass;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::{
    cell::{
        Cell,
        RefCell,
    },
    collections::HashMap,
    convert::TryFrom,
    ffi::OsStr,
    fs::{self, File},
    path::{
        Path,
    },
    rc::Rc,
    str,
};

use crate::{
    DaemonBoard,
    DerefCell,
    KeyboardColorButton,
    KeyMap,
};
use super::{
    error_dialog,
    Key,
    KeyboardLayer,
    Layout,
    Page,
    Picker,
};

#[derive(Default)]
pub struct KeyboardInner {
    action_group: DerefCell<gio::SimpleActionGroup>,
    board: DerefCell<DaemonBoard>,
    board_name: DerefCell<String>,
    default_layout: DerefCell<KeyMap>,
    keymap: DerefCell<HashMap<String, u16>>,
    keys: DerefCell<Rc<[Key]>>,
    page: Cell<Page>,
    picker: RefCell<WeakRef<Picker>>,
    selected: Cell<Option<usize>>,
    color_button_bin: DerefCell<gtk::Frame>,
    brightness_scale: DerefCell<gtk::Scale>,
    stack: DerefCell<gtk::Stack>,
}

impl ObjectSubclass for KeyboardInner {
    const NAME: &'static str = "S76Keyboard";

    type ParentType = gtk::Box;
    type Type = Keyboard;
    type Interfaces = ();

    type Instance = subclass::simple::InstanceStruct<Self>;
    type Class = subclass::simple::ClassStruct<Self>;

    glib::object_subclass!();

    fn new() -> Self {
        Self::default()
    }
}

impl ObjectImpl for KeyboardInner {
    fn constructed(&self, keyboard: &Keyboard) {
        self.parent_constructed(keyboard);

        let stack = cascade! {
            gtk::Stack::new();
            ..set_transition_duration(0);
            ..connect_property_visible_child_notify(
                clone!(@weak keyboard => move |stack| {
                    let page = stack
                        .get_visible_child()
                        .map(|c| c.downcast_ref::<KeyboardLayer>().unwrap().page());

                    println!("{:?}", page);
                    let last_layer = keyboard.layer();
                    keyboard.inner().page.set(page.unwrap_or(Page::Layer1));
                    if keyboard.layer() != last_layer {
                        keyboard.set_selected(keyboard.selected());
                    }
                })
            );
        };

        let brightness_scale = cascade! {
            gtk::Scale::with_range(gtk::Orientation::Horizontal, 0., 100., 1.);
            ..set_halign(gtk::Align::Fill);
            ..set_size_request(200, 0);
        };
        brightness_scale.connect_value_changed(
            clone!(@weak keyboard => move |this| {
                let value = this.get_value() as i32;
                if let Err(err) = keyboard.board().set_brightness(value) {
                    eprintln!("{}", err);
                }
                println!("{}", value);
            })
        );

        // XXX add support to ColorButton for changing keyboard
        let color_button_bin = cascade! {
            gtk::Frame::new(None);
            ..set_shadow_type(gtk::ShadowType::None);
            ..set_valign(gtk::Align::Center);
        };

        cascade! {
            keyboard;
            ..set_orientation(gtk::Orientation::Vertical);
            ..set_spacing(8);
            ..add(&cascade! {
                gtk::Box::new(gtk::Orientation::Horizontal, 8);
                ..add(&cascade! {
                    gtk::Label::new(Some("Brightness:"));
                    ..set_halign(gtk::Align::Start);
                });
                ..add(&brightness_scale);
                ..add(&cascade! {
                    gtk::Label::new(Some("Color:"));
                    ..set_halign(gtk::Align::Start);
                });
                ..add(&color_button_bin);
            });
            ..add(&stack);
        };

        let action_group = gio::SimpleActionGroup::new();
        action_group.add_action(&cascade! {
            gio::SimpleAction::new("load", None);
            ..connect_activate(clone!(@weak keyboard => move |_, _| {
                keyboard.load();
            }));
        });
        action_group.add_action(&cascade! {
            gio::SimpleAction::new("save", None);
            ..connect_activate(clone!(@weak keyboard => move |_, _| {
                keyboard.save();
            }));
        });
        action_group.add_action(&cascade! {
            gio::SimpleAction::new("reset", None);
            ..connect_activate(clone!(@weak keyboard => move |_, _| {
                keyboard.reset();
            }));
        });

        self.action_group.set(action_group);
        self.color_button_bin.set(color_button_bin);
        self.brightness_scale.set(brightness_scale);
        self.stack.set(stack);
    }

    fn properties() -> &'static [glib::ParamSpec] {
        use once_cell::sync::Lazy;
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![
                glib::ParamSpec::int(
                    "selected",
                    "selected",
                    "selected",
                    -1,
                    i32::MAX,
                    -1,
                    glib::ParamFlags::READWRITE,
                )
            ]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, keyboard: &Keyboard, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        match pspec.get_name() {
            "selected" => {
                let v: i32 = value.get_some().unwrap();
                let selected = usize::try_from(v).ok();
                keyboard.set_selected(selected);
            }
            _ => unimplemented!(),
        }
    }

    fn get_property(&self, keyboard: &Keyboard, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.get_name() {
            "selected" => {
                keyboard.selected().map(|v| v as i32).unwrap_or(-1).to_value()
            }
            _ => unimplemented!(),
        }
    }
}

impl WidgetImpl for KeyboardInner {}
impl ContainerImpl for KeyboardInner {}
impl BoxImpl for KeyboardInner {}

glib::wrapper! {
    pub struct Keyboard(ObjectSubclass<KeyboardInner>)
        @extends gtk::Box, gtk::Container, gtk::Widget, @implements gtk::Orientable;
}

impl Keyboard {
    #[allow(dead_code)]
    pub fn new<P: AsRef<Path>>(dir: P, board_name: &str, board: DaemonBoard) -> Self {
        let dir = dir.as_ref();

        let default_json = fs::read_to_string(dir.join("default_json"))
            .expect("Failed to load keymap.json");
        let keymap_json = fs::read_to_string(dir.join("keymap.json"))
            .expect("Failed to load keymap.json");
        let layout_json = fs::read_to_string(dir.join("layout.json"))
            .expect("Failed to load layout.json");
        let physical_json = fs::read_to_string(dir.join("physical.json"))
            .expect("Failed to load physical.json");
        Self::new_data(board_name, &default_json, &keymap_json, &layout_json, &physical_json, board)
    }

    fn new_layout(board_name: &str, layout: Layout, board: DaemonBoard) -> Self {
        let keyboard: Self = glib::Object::new(&[]).unwrap();

        let mut keys = layout.keys();
        for key in keys.iter_mut() {
            for layer in 0..2 {
                println!("  Layer {}", layer);
                let scancode = match board.keymap_get(layer, key.electrical.0, key.electrical.1) {
                    Ok(value) => value,
                    Err(err) => {
                        eprintln!("Failed to read scancode: {:?}", err);
                        0
                    }
                };
                println!("    Scancode: {:04X}", scancode);

                let scancode_name = match layout.scancode_names.get(&scancode) {
                    Some(some) => some.to_string(),
                    None => String::new(),
                };
                println!("    Scancode Name: {}", scancode_name);

                key.scancodes.borrow_mut().push((scancode, scancode_name));
            }
        }

        keyboard.inner().keys.set(keys.into_boxed_slice().into());
        keyboard.inner().board.set(board);
        keyboard.inner().board_name.set(board_name.to_string());
        keyboard.inner().keymap.set(layout.keymap);
        keyboard.inner().default_layout.set(layout.default);

        let color_button = KeyboardColorButton::new(keyboard.board().clone());
        keyboard.inner().color_button_bin.add(&color_button);

        let max_brightness = match keyboard.board().max_brightness() {
            Ok(value) => value as f64,
            Err(err) => {
                eprintln!("{}", err);
                100.0
            }
        };
        keyboard.inner().brightness_scale.set_range(0.0, max_brightness);

        let brightness = match keyboard.board().brightness() {
            Ok(value) => value as f64,
            Err(err) => {
                eprintln!("{}", err);
                0.0
            }
        };
        keyboard.inner().brightness_scale.set_value(brightness);

        keyboard.add_pages();

        keyboard
    }

    pub fn new_board(board_name: &str, board: DaemonBoard) -> Option<Self> {
        Layout::from_board(board_name).map(|layout|
            Self::new_layout(board_name, layout, board)
        )
    }

    #[allow(dead_code)]
    fn new_data(board_name: &str, default_json: &str, keymap_json: &str, layout_json: &str, physical_json: &str, board: DaemonBoard) -> Self {
        let layout = Layout::from_data(default_json, keymap_json, layout_json, physical_json);
        Self::new_layout(board_name, layout, board)
    }

    fn inner(&self) -> &KeyboardInner {
        KeyboardInner::from_instance(self)
    }

    pub fn action_group(&self) -> &gio::ActionGroup {
        self.inner().action_group.upcast_ref()
    }

    fn board_name(&self) -> &str {
        &self.inner().board_name
    }

    fn board(&self) -> &DaemonBoard {
        &self.inner().board
    }

    fn keymap(&self) -> &HashMap<String, u16> {
        &self.inner().keymap
    }

    fn default_layout(&self) -> &KeyMap {
        &self.inner().default_layout
    }

    fn window(&self) -> Option<gtk::Window> {
        self.get_toplevel()?.downcast().ok()
    }

    pub fn layer(&self) -> Option<usize> {
        match self.inner().page.get() {
            Page::Layer1 => Some(0),
            Page::Layer2 => Some(1),
            _ => None
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.inner().selected.get()
    }

    pub fn stack(&self) -> &gtk::Stack {
        &self.inner().stack
    }

    pub fn has_scancode(&self, scancode_name: &str) -> bool {
        self.keymap().contains_key(scancode_name)
    }

    pub fn keys(&self) -> &Rc<[Key]> {
        &self.inner().keys
    }

    pub fn keymap_set(&self, key_index: usize, layer: usize, scancode_name: &str) {
        let k = &self.keys()[key_index];
        let mut found = false;
        if let Some(scancode) = self.keymap().get(scancode_name) {
            k.scancodes.borrow_mut()[layer] = (*scancode, scancode_name.to_string());
            found = true;
        }
        if !found {
            return;
        }
        println!(
            "  set {}, {}, {} to {:04X}",
            layer, k.electrical.0, k.electrical.1, k.scancodes.borrow()[layer].0
        );
        if let Err(err) = self.board().keymap_set(
            layer as u8,
            k.electrical.0,
            k.electrical.1,
            k.scancodes.borrow_mut()[layer].0,
        ) {
            eprintln!("Failed to set keymap: {:?}", err);
        }

        self.set_selected(self.selected());
    }

    pub fn export_keymap(&self) -> KeyMap {
        let mut map = HashMap::new();
        for key in self.keys().iter() {
            let scancodes = key.scancodes.borrow();
            let scancodes = scancodes.iter().map(|s| s.1.clone()).collect();
            map.insert(key.logical_name.clone(), scancodes);
        }
        KeyMap {
            board: self.board_name().to_string(),
            map: map,
        }
    }

    pub fn import_keymap(&self, keymap: &KeyMap) {
        // TODO: don't block UI thread
        // TODO: Ideally don't want this function to be O(Keys^2)

        if &keymap.board != self.board_name() {
            error_dialog(&self.window().unwrap(),
                         "Failed to import keymap",
                         format!("Keymap is for board '{}'", keymap.board));
            return;
        }

        for (k, v) in keymap.map.iter() {
            let n = self
                .keys()
                .iter()
                .position(|i| &i.logical_name == k)
                .unwrap();
            for (layer, scancode_name) in v.iter().enumerate() {
                self.keymap_set(n, layer, scancode_name);
            }
        }
    }

    fn load(&self) {
        let filter = cascade! {
            gtk::FileFilter::new();
            ..set_name(Some("JSON"));
            ..add_pattern("*.json");
        };

        let chooser = cascade! {
            gtk::FileChooserNative::new::<gtk::Window>(Some("Load Layout"), None, gtk::FileChooserAction::Open, Some("Load"), Some("Cancel"));
            ..add_filter(&filter);
        };

        if chooser.run() == gtk::ResponseType::Accept {
            let path = chooser.get_filename().unwrap();
            match File::open(&path) {
                Ok(file) => match KeyMap::from_reader(file) {
                    Ok(keymap) => self.import_keymap(&keymap),
                    Err(err) => error_dialog(&self.window().unwrap(), "Failed to import keymap", err),
                }
                Err(err) => error_dialog(&self.window().unwrap(), "Failed to open file", err),
            }
        }
    }

    fn save(&self) {
        let filter = cascade! {
            gtk::FileFilter::new();
            ..set_name(Some("JSON"));
            ..add_pattern("*.json");
        };

        let chooser = cascade! {
            gtk::FileChooserNative::new::<gtk::Window>(Some("Save Layout"), None, gtk::FileChooserAction::Save, Some("Save"), Some("Cancel"));
            ..add_filter(&filter);
        };

        if chooser.run() == gtk::ResponseType::Accept {
            let mut path = chooser.get_filename().unwrap();
            match path.extension() {
                None => { path.set_extension(OsStr::new("json")); }
                Some(ext) if ext == OsStr::new("json") => {}
                Some(ext) => {
                    let mut ext = ext.to_owned();
                    ext.push(".json");
                    path.set_extension(&ext);
                }
            }
            let keymap = self.export_keymap();

            match File::create(&path) {
                Ok(file) => match keymap.to_writer_pretty(file) {
                    Ok(()) => {},
                    Err(err) => error_dialog(&self.window().unwrap(), "Failed to export keymap", err),
                }
                Err(err) => error_dialog(&self.window().unwrap(), "Failed to open file", err),
            }
        }
    }

    fn reset(&self) {
        self.import_keymap(self.default_layout());
    }

    fn add_pages(&self) {
        let stack = &*self.inner().stack;

        for (i, page) in Page::iter_all().enumerate() {
            let keyboard_layer = KeyboardLayer::new(page, self.keys().clone());
            self.bind_property("selected", &keyboard_layer, "selected")
                .flags(glib::BindingFlags::BIDIRECTIONAL)
                .build();
            stack.add_titled(&keyboard_layer, page.name(), page.name());

            self.inner().action_group.add_action(&cascade! {
                gio::SimpleAction::new(&format!("page{}", i), None);
                ..connect_activate(clone!(@weak stack, @weak keyboard_layer => move |_, _| {
                    stack.set_visible_child(&keyboard_layer);
                }));
            });
        }
    }

    pub(super) fn set_picker(&self, picker: Option<&Picker>) {
        // This function is called by Picker::set_keyboard()
        *self.inner().picker.borrow_mut() = match picker {
            Some(picker) => {
                picker.set_sensitive(self.selected().is_some() && self.layer() != None);
                picker.downgrade()
            },
            None => WeakRef::new(),
        };
    }

    fn set_selected(&self, i: Option<usize>) {
        let picker = match self.inner().picker.borrow().upgrade() {
            Some(picker) => picker,
            None => { return; },
        };
        let keys = self.keys();

        picker.set_selected(None);

        if let Some(i) = i {
            let k = &keys[i];
            println!("{:#?}", k);
            if let Some(layer) = self.layer() {
                if let Some((_scancode, scancode_name)) = keys[i].scancodes.borrow().get(layer) {
                    picker.set_selected(Some(scancode_name.to_string()));
                }
            }
        }

        picker.set_sensitive(i.is_some() && self.layer() != None);

        self.inner().selected.set(i);

        self.queue_draw();
        self.notify("selected");
    }
}
