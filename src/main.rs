use adw::prelude::*;
use adw::{
    Application, ApplicationWindow, Breakpoint, BreakpointCondition, HeaderBar, OverlaySplitView,
    StatusPage, Toast, ToastOverlay, ToolbarView, WindowTitle,
};
use adw::gtk::{Button, Label, ListBox, SelectionMode, ToggleButton};

const APP_ID: &str = "in.tl2a.kopico-desktop";

// (title, icon) for each sidebar entry
const PAGES: [(&str, &str); 3] = [
    ("Home", "go-home-symbolic"),
    ("Documents", "folder-documents-symbolic"),
    ("Settings", "emblem-system-symbolic"),
];

fn main() -> gtk4::glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    // ---- Sidebar ----
    let sidebar_list = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .css_classes(["navigation-sidebar"])
        .build();
    for (title, _) in PAGES {
        let label = Label::builder()
            .label(title)
            .xalign(0.0)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .build();
        sidebar_list.append(&label);
    }

    let sidebar_page = ToolbarView::new();
    sidebar_page.add_top_bar(&HeaderBar::builder().title_widget(&WindowTitle::new("Kopico", "")).build());
    sidebar_page.set_content(Some(&sidebar_list));

    // ---- Content ----
    let window_title = WindowTitle::new(PAGES[0].0, "");

    let sidebar_toggle = ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text("Toggle Sidebar")
        .active(true)
        .build();

    let content_header = HeaderBar::builder().title_widget(&window_title).build();
    content_header.pack_start(&sidebar_toggle);

    let button = Button::builder()
        .label("Click me!")
        .halign(gtk4::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();

    let status_page = StatusPage::builder()
        .icon_name(PAGES[0].1)
        .title(PAGES[0].0)
        .description("Pick a page from the sidebar, or press the button.")
        .child(&button)
        .build();

    let toast_overlay = ToastOverlay::new();
    toast_overlay.set_child(Some(&status_page));

    let content_page = ToolbarView::new();
    content_page.add_top_bar(&content_header);
    content_page.set_content(Some(&toast_overlay));

    // ---- Split view ----
    let split_view = OverlaySplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .show_sidebar(true)
        .build();

    // Keep the header toggle and the sidebar visibility in sync both ways
    split_view
        .bind_property("show-sidebar", &sidebar_toggle, "active")
        .bidirectional()
        .sync_create()
        .build();

    // ---- Behaviour ----
    {
        let toast_overlay = toast_overlay.clone();
        let window_title = window_title.clone();
        button.connect_clicked(move |_| {
            let page = window_title.title();
            println!("Button clicked on the {page} page!");
            toast_overlay.add_toast(Toast::new(&format!("Button clicked on {page}")));
        });
    }

    {
        let split_view = split_view.clone();
        sidebar_list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let (title, icon) = PAGES[row.index() as usize];
            println!("Switched to {title}");
            window_title.set_title(title);
            status_page.set_title(title);
            status_page.set_icon_name(Some(icon));

            // On narrow windows the sidebar overlays the content, so hide it after picking
            if split_view.is_collapsed() {
                split_view.set_show_sidebar(false);
            }
        });
    }
    sidebar_list.select_row(sidebar_list.row_at_index(0).as_ref());

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Kopico Desktop")
        .default_width(800)
        .default_height(500)
        .width_request(360)
        .height_request(294)
        .content(&split_view)
        .build();

    // Collapse the sidebar into an overlay when the window gets narrow
    let breakpoint = Breakpoint::new(BreakpointCondition::parse("max-width: 600sp").unwrap());
    breakpoint.add_setter(&split_view, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(breakpoint);

    window.present();
}
