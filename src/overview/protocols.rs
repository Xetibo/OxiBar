macro_rules! protocol {
    ($module:ident, $xml:literal) => {
        #[allow(
            dead_code,
            non_camel_case_types,
            non_upper_case_globals,
            non_snake_case,
            unused_imports,
            unused_unsafe,
            unused_variables,
            clippy::all
        )]
        pub mod $module {
            use wayland_client;
            use wayland_client::protocol::*;
            use wayland_protocols::ext::foreign_toplevel_list::v1::client::*;
            use wayland_protocols_wlr::foreign_toplevel::v1::client::*;
            pub mod __interfaces {
                use wayland_client::backend as wayland_backend;
                use wayland_client::protocol::__interfaces::*;
                use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
                use wayland_protocols_wlr::foreign_toplevel::v1::client::__interfaces::*;
                wayland_scanner::generate_interfaces!($xml);
            }
            use self::__interfaces::*;
            wayland_scanner::generate_client_code!($xml);
        }
    };
}
protocol!(
    export,
    "src/overview/protocols/hyprland-toplevel-export-v1.xml"
);
protocol!(
    mapping,
    "src/overview/protocols/hyprland-toplevel-mapping-v1.xml"
);

#[cfg(test)]
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    unused_imports,
    unused_unsafe,
    unused_variables,
    clippy::all
)]
pub mod server_mapping {
    use wayland_protocols::ext::foreign_toplevel_list::v1::server::*;
    use wayland_protocols_wlr::foreign_toplevel::v1::server::*;
    use wayland_server;
    use wayland_server::protocol::*;
    pub mod __interfaces {
        use wayland_protocols::ext::foreign_toplevel_list::v1::server::__interfaces::*;
        use wayland_protocols_wlr::foreign_toplevel::v1::server::__interfaces::*;
        use wayland_server::backend as wayland_backend;
        use wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "src/overview/protocols/hyprland-toplevel-mapping-v1.xml"
        );
    }
    use self::__interfaces::*;
    wayland_scanner::generate_server_code!(
        "src/overview/protocols/hyprland-toplevel-mapping-v1.xml"
    );
}
