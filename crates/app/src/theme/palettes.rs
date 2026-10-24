//! The four Catppuccin flavors (MIT license, github.com/catppuccin/catppuccin).

/// One flavor. `accents` follow [`ACCENT_NAMES`].
pub struct Flavor {
    pub accents: [u32; 14],
    pub text: u32,
    pub subtext1: u32,
    pub overlay2: u32,
    pub overlay0: u32,
    pub surface2: u32,
    pub surface1: u32,
    pub surface0: u32,
    pub base: u32,
    pub mantle: u32,
    pub crust: u32,
}

pub const FLAVOR_NAMES: [&str; 4] = ["Latte", "Frappé", "Macchiato", "Mocha"];

pub const ACCENT_NAMES: [&str; 14] = [
    "Rosewater",
    "Flamingo",
    "Pink",
    "Mauve",
    "Red",
    "Maroon",
    "Peach",
    "Yellow",
    "Green",
    "Teal",
    "Sky",
    "Sapphire",
    "Blue",
    "Lavender",
];

/// The flavor at `index` of [`FLAVOR_NAMES`]; out of range gives Mocha.
pub fn flavor(index: usize) -> &'static Flavor {
    FLAVORS.get(index).unwrap_or(&FLAVORS[3])
}

const FLAVORS: [Flavor; 4] = [
    Flavor {
        accents: [
            0xdc8a78, 0xdd7878, 0xea76cb, 0x8839ef, 0xd20f39, 0xe64553, 0xfe640b, 0xdf8e1d,
            0x40a02b, 0x179299, 0x04a5e5, 0x209fb5, 0x1e66f5, 0x7287fd,
        ],
        text: 0x4c4f69,
        subtext1: 0x5c5f77,
        overlay2: 0x7c7f93,
        overlay0: 0x9ca0b0,
        surface2: 0xacb0be,
        surface1: 0xbcc0cc,
        surface0: 0xccd0da,
        base: 0xeff1f5,
        mantle: 0xe6e9ef,
        crust: 0xdce0e8,
    },
    Flavor {
        accents: [
            0xf2d5cf, 0xeebebe, 0xf4b8e4, 0xca9ee6, 0xe78284, 0xea999c, 0xef9f76, 0xe5c890,
            0xa6d189, 0x81c8be, 0x99d1db, 0x85c1dc, 0x8caaee, 0xbabbf1,
        ],
        text: 0xc6d0f5,
        subtext1: 0xb5bfe2,
        overlay2: 0x949cbb,
        overlay0: 0x737994,
        surface2: 0x626880,
        surface1: 0x51576d,
        surface0: 0x414559,
        base: 0x303446,
        mantle: 0x292c3c,
        crust: 0x232634,
    },
    Flavor {
        accents: [
            0xf4dbd6, 0xf0c6c6, 0xf5bde6, 0xc6a0f6, 0xed8796, 0xee99a0, 0xf5a97f, 0xeed49f,
            0xa6da95, 0x8bd5ca, 0x91d7e3, 0x7dc4e4, 0x8aadf4, 0xb7bdf8,
        ],
        text: 0xcad3f5,
        subtext1: 0xb8c0e0,
        overlay2: 0x939ab7,
        overlay0: 0x6e738d,
        surface2: 0x5b6078,
        surface1: 0x494d64,
        surface0: 0x363a4f,
        base: 0x24273a,
        mantle: 0x1e2030,
        crust: 0x181926,
    },
    Flavor {
        accents: [
            0xf5e0dc, 0xf2cdcd, 0xf5c2e7, 0xcba6f7, 0xf38ba8, 0xeba0ac, 0xfab387, 0xf9e2af,
            0xa6e3a1, 0x94e2d5, 0x89dceb, 0x74c7ec, 0x89b4fa, 0xb4befe,
        ],
        text: 0xcdd6f4,
        subtext1: 0xbac2de,
        overlay2: 0x9399b2,
        overlay0: 0x6c7086,
        surface2: 0x585b70,
        surface1: 0x45475a,
        surface0: 0x313244,
        base: 0x1e1e2e,
        mantle: 0x181825,
        crust: 0x11111b,
    },
];
