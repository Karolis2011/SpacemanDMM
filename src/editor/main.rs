//! The main crate for the map editor.

#[macro_use] extern crate qt_extras as qt;
#[macro_use] extern crate glium;

extern crate dreammaker as dm;
extern crate dmm_tools;
extern crate same_file;

mod map_renderer;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use dm::objtree::{ObjectTree, TypeRef};

use qt::widgets;
use qt::widgets::widget::Widget;
use qt::widgets::application::Application;
use qt::widgets::file_dialog::FileDialog;
use qt::widgets::tree_widget::TreeWidget;
use qt::widgets::tree_widget_item::TreeWidgetItem;
use qt::gui::key_sequence::KeySequence;
use qt::core::connection::Signal;
use qt::core::slots::SlotNoArgs;
use qt::core::flags::Flags;
use qt::cpp_utils::StaticCast;
use qt::cpp_utils::static_cast_mut;

use same_file::is_same_file;

fn show_error(window: &mut Widget, message: &str) {
    use qt::widgets::message_box::*;
    unsafe {
        MessageBox::critical((
            window as *mut Widget,
            qstr!("Error"),
            qstr!(message),
            Flags::from_enum(StandardButton::Ok),
        ));
    }
}

struct Map {
    path: PathBuf,
    dmm: dmm_tools::dmm::Map,
}

struct State {
    environment_file: Option<PathBuf>,
    objtree: Option<dm::objtree::ObjectTree>,
    maps: Vec<Map>,
    current_map: usize,
}

impl State {
    fn new() -> State {
        State {
            environment_file: None,
            objtree: None,
            maps: Vec::new(),
            current_map: 0,
        }
    }

    unsafe fn load_env(&mut self, path: PathBuf, window: &mut Widget, widget: &mut TreeWidget) {
        println!("Environment: {}", path.display());

        let mut preprocessor;
        match dm::preprocessor::Preprocessor::new(path.clone()) {
            Err(_) => return show_error(window, &format!("Could not open for reading:\n{}", path.display())),
            Ok(pp) => preprocessor = pp,
        };

        let objtree;
        match dm::parser::parse(dm::indents::IndentProcessor::new(&mut preprocessor)) {
            Err(e) => {
                let mut message = format!("\
                    Could not parse the environment:\n\
                    {}\n\n\
                    This may be caused by incorrect or unusual code, but is typically a parser bug. \
                    Change the code to use a more common form, or report the parsing problem.\n\
                ", path.display());
                let mut message_buf = Vec::new();
                let _ = dm::pretty_print_error(&mut message_buf, &preprocessor, &e);
                message.push_str(&String::from_utf8_lossy(&message_buf[..]));
                return show_error(window, &message);
            },
            Ok(t) => objtree = t,
        }

        self.environment_file = Some(path);
        {
            widget.clear();
            let root = objtree.root();
            for &root_child in ["area", "turf", "obj", "mob"].iter() {
                let ty = root.child(root_child, &objtree).expect("builtins missing");

                let mut root_item = TreeWidgetItem::new(());
                root_item.set_text(0, qstr!(&ty.name));
                add_children(&mut root_item, ty, &objtree);
                widget.add_top_level_item(qt_own!(root_item));
            }
        }
        self.objtree = Some(objtree);
    }

    unsafe fn load_map(&mut self, path: PathBuf, window: &mut Widget) {
        println!("Map: {}", path.display());

        // Verify that we're in the right environment
        let env = detect_environment(&path);
        println!("Detect: {:?}", env);
        match (env, self.environment_file.as_ref()) {
            (Some(env), Some(real_env)) => if !is_same_file(env, real_env).unwrap_or(false) { return },
            _ => return,
        }

        let map = match dmm_tools::dmm::Map::from_file(&path) {
            Err(e) => {
                let message = format!("Could not load the map:\n{}\n\n{}", path.display(), e.description());
                return show_error(window, &message);
            }
            Ok(map) => map,
        };

        println!("Success");
        self.maps.push(Map {
            path: path,
            dmm: map,
        });
    }

    unsafe fn close_map(&mut self, index: usize) {
        if index >= self.maps.len() { return }
        self.maps.remove(index);
    }
}

unsafe fn add_children(parent: &mut TreeWidgetItem, ty: TypeRef, tree: &ObjectTree) {
    let mut children = ty.children(tree);
    children.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    for each in children {
        let mut child = TreeWidgetItem::new(());
        child.set_text(0, qstr!(&each.name));
        add_children(&mut child, each, tree);
        parent.add_child(qt_own!(child));
    }
}

macro_rules! action {
    (@[$it:ident] (tip = $text:expr)) => {
        $it.set_status_tip(&qstr!($text));
    };
    (@[$it:ident] (key = $(^$m:ident)* $k:ident)) => {
        $it.set_shortcut(&KeySequence::new( qt::core::qt::Key::$k as i32 $(+ qt::core::qt::Modifier::$m as i32)* ));
    };
    (@[$it:ident] (slot = $slot:expr)) => {
        $it.signals().triggered().connect(&$slot);
    };
    (@[$it:ident] $closure:block) => {
        let slot = SlotNoArgs::new(|| $closure);
        $it.signals().triggered().connect(&slot);
    };
    ($add_to:expr, $name:expr $(, $x:tt)*) => {
        let it = &mut *$add_to.add_action(qstr!($name));
        $(action!(@[it] $x);)*
    }
}

fn detect_environment(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(dir) = current {
        let read_dir = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return None,
        };
        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => return None,
            };
            let path = entry.path();
            if path.extension() == Some("dme".as_ref()) {
                return Some(path);
            }
        }
        current = dir.parent();
    }
    None
}

#[allow(unused_mut)]
fn main() {
    let mut state = RefCell::new(State::new());

    // Determine the configuration directory
    let mut config_dir;
    if let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") {
        // If we're being run through Cargo, put runtime files in target/
        config_dir = PathBuf::from(manifest_dir);
        config_dir.push("target");
    } else if let Ok(current_exe) = std::env::current_exe() {
        // Otherwise, put runtime files adjacent to the executable
        config_dir = current_exe;
        config_dir.pop();
    } else {
        // As a fallback, use the working directory
        config_dir = PathBuf::from(".");
    }

    // Initialize the GUI
    Application::create_and_exit(|_app| unsafe {
        let mut window = widgets::main_window::MainWindow::new();
        let window_ptr = window.as_mut_ptr();

        // object tree
        let mut tree_widget = widgets::tree_widget::TreeWidget::new();
        let tree_widget_ptr = tree_widget.as_mut_ptr();
        tree_widget.set_column_count(1);
        tree_widget.set_header_hidden(true);

        // map tabs
        let mut map_tabs = qt::widgets::tab_bar::TabBar::new();
        let map_tabs_ptr = map_tabs.as_mut_ptr();
        map_tabs.set_tabs_closable(true);
        map_tabs.set_expanding(false);
        map_tabs.set_document_mode(true);
        let tab_close_slot = qt::core::slots::SlotCInt::new(|idx| {
            state.borrow_mut().close_map(idx as usize);
        });
        map_tabs.signals().tab_close_requested().connect(&tab_close_slot);
        let tab_select_slot = qt::core::slots::SlotCInt::new(|idx| {
            println!("select {}", idx);
        });
        map_tabs.signals().current_changed().connect(&tab_select_slot);

        // minimap
        let mut minimap_widget = qt::glium_widget::create(map_renderer::GliumTest);
        minimap_widget.set_minimum_size((256, 256));
        minimap_widget.set_maximum_size((256, 256));

        // tools
        let mut tools = widgets::label::Label::new(qstr!("Tools Go Here"));

        // instances
        let mut list_view = widgets::list_view::ListView::new();

        // map
        let mut map_widget = qt::glium_widget::create(map_renderer::GliumTest);

        // the layouts
        let mut tools_layout = widgets::v_box_layout::VBoxLayout::new();
        tools_layout.add_widget(qt_own!(minimap_widget));
        tools_layout.add_widget(qt_own!(tools));
        tools_layout.add_widget(qt_own!(list_view));

        let mut h_layout = widgets::h_box_layout::HBoxLayout::new();
        h_layout.set_spacing(5);
        h_layout.add_layout(qt_own!(tools_layout));
        h_layout.add_widget((qt_own!(map_widget), 1));
        h_layout.set_contents_margins((0, 0, 0, 0));

        let mut tabbed_layout = widgets::v_box_layout::VBoxLayout::new();
        tabbed_layout.set_spacing(0);
        tabbed_layout.add_widget(qt_own!(map_tabs));
        tabbed_layout.add_layout((qt_own!(h_layout), 1));
        tabbed_layout.set_contents_margins((0, 0, 0, 0));

        let mut h_layout_widget = widgets::widget::Widget::new();
        h_layout_widget.set_layout(qt_own!(tabbed_layout));

        // root splitter
        let mut splitter = widgets::splitter::Splitter::new(());
        splitter.set_children_collapsible(false);
        splitter.add_widget(qt_own!(tree_widget));
        splitter.add_widget(qt_own!(h_layout_widget));
        splitter.set_stretch_factor(0, 0);
        splitter.set_stretch_factor(1, 1);

        // menus
        let mut menu_bar = widgets::menu_bar::MenuBar::new();
        // file menu
        let mut menu_file = &mut *menu_bar.add_menu(qstr!("File"));
        action!(menu_file, "Open Environment", (tip = "Load a DME file."), {
            let file = FileDialog::get_open_file_name_unsafe((
                static_cast_mut(window_ptr),
                qstr!("Open Environment"),
                qstr!("."),
                qstr!("Environments (*.dme)"),
            )).to_std_string();
            if !file.is_empty() {
                state.borrow_mut().load_env(PathBuf::from(file), (*window_ptr).static_cast_mut(), &mut *tree_widget_ptr);
            }
        });
        menu_file.add_menu(qstr!("Recent Environments"));
        menu_file.add_separator();
        action!(menu_file, "New", (key = ^CTRL KeyN), (tip = "Create a new map."), {
            let map_tabs = &mut *map_tabs_ptr;
            map_tabs.add_tab(qstr!("New Map"));
        });
        action!(menu_file, "Open", (key = ^CTRL KeyO), (tip = "Open a map."), {
            let file = FileDialog::get_open_file_name_unsafe((
                static_cast_mut(window_ptr),
                qstr!("Open Map"),
                qstr!(&match state.borrow().environment_file.as_ref().and_then(|x| x.parent()).and_then(|x| x.to_str()) {
                    Some(dir) => dir,
                    None => ".",
                }),
                qstr!("Maps (*.dmm)"),
            )).to_std_string();
            if !file.is_empty() {
                state.borrow_mut().load_map(PathBuf::from(file), (*window_ptr).static_cast_mut());
            }
        });
        action!(menu_file, "Close", (key = ^CTRL KeyW), (tip = "Close the current map."), {
            let mut state = state.borrow_mut();
            let map = state.current_map;
            state.close_map(map);
        });
        menu_file.add_separator();
        action!(menu_file, "Exit", (key = ^ALT KeyF4), (slot = window.slots().close()));

        // help menu
        let mut menu_help = &mut *menu_bar.add_menu(qstr!("Help"));
        action!(menu_help, "User Guide", (key = KeyF1));
        action!(menu_help, "About", {
            use qt::widgets::message_box::*;
            let mut mbox = MessageBox::new((
                Icon::Information,
                qstr!("About SpacemanDMM"),
                qstr!(concat!(
                    "SpacemanDMM v", env!("CARGO_PKG_VERSION"), "\n",
                    "by SpaceManiac, for /tg/station13",
                )),
                Flags::from_enum(StandardButton::Ok),
            ));
            {
                let widget: &mut Widget = mbox.static_cast_mut();
                widget.set_attribute(qt::core::qt::WidgetAttribute::DeleteOnClose);
            }
            mbox.show();
            mbox.into_raw();
        });

        // status bar
        let mut status_bar = widgets::status_bar::StatusBar::new();

        // build main window
        window.set_window_title(qstr!("SpacemanDMM"));
        window.resize((1400, 768));

        window.set_menu_bar(qt_own!(menu_bar));
        window.set_status_bar(qt_own!(status_bar));
        window.set_central_widget(qt_own!(splitter));
        window.show();

        // parse command-line arguments:
        // - use the specified DME, or autodetect one from the first DMM
        // - preload all maps specified belonging to that DME
        let mut preload_maps = Vec::new();
        for arg in std::env::args_os() {
            let mut state = state.borrow_mut();
            let path = PathBuf::from(arg);

            if path.extension() == Some("dme".as_ref()) {
                if state.environment_file.is_some() {
                    // only one DME may be specified
                    continue;
                }
                state.load_env(path, (*window_ptr).static_cast_mut(), &mut *tree_widget_ptr);
            } else if path.extension() == Some("dmm".as_ref()) {
                // determine the corresponding DME
                let detected_env = match detect_environment(&path) {
                    Some(env) => env,
                    None => continue,
                };

                if let Some(env_file) = state.environment_file.as_ref() {
                    if !is_same_file(env_file, detected_env).unwrap_or(false) {
                        preload_maps.push(path);
                    }
                    continue;
                }

                state.load_env(detected_env, (*window_ptr).static_cast_mut(), &mut *tree_widget_ptr);
                preload_maps.push(path);
            } else {
                continue;
            }
        }

        // TODO: If no DME is loaded, attempt to open the most recent one, failing silently
        for map in preload_maps {
            state.borrow_mut().load_map(map, (*window_ptr).static_cast_mut());
        }

        // cede control
        Application::exec()
    })
}