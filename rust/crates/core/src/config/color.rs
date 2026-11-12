use super::x11_colors::X11_COLORS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub type Palette = [Rgb; 256];

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

const ANSI: [Rgb; 16] = [
    rgb(0x1D, 0x1F, 0x21),
    rgb(0xCC, 0x66, 0x66),
    rgb(0xB5, 0xBD, 0x68),
    rgb(0xF0, 0xC6, 0x74),
    rgb(0x81, 0xA2, 0xBE),
    rgb(0xB2, 0x94, 0xBB),
    rgb(0x8A, 0xBE, 0xB7),
    rgb(0xC5, 0xC8, 0xC6),
    rgb(0x66, 0x66, 0x66),
    rgb(0xD5, 0x4E, 0x53),
    rgb(0xB9, 0xCA, 0x4A),
    rgb(0xE7, 0xC5, 0x47),
    rgb(0x7A, 0xA6, 0xDA),
    rgb(0xC3, 0x97, 0xD8),
    rgb(0x70, 0xC0, 0xB1),
    rgb(0xEA, 0xEA, 0xEA),
];

/// The built-in 256-color palette: 16 named colors, a 6x6x6 cube and a gray ramp.
pub fn default_palette() -> Palette {
    let mut result = [rgb(0, 0, 0); 256];
    result[..16].copy_from_slice(&ANSI);
    let level = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
    let mut i = 16;
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                result[i] = rgb(level(r), level(g), level(b));
                i += 1;
            }
        }
    }
    for step in 0..24u8 {
        let v = step * 10 + 8;
        result[232 + step as usize] = rgb(v, v, v);
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorParseError;

impl std::fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid color format")
    }
}

impl std::error::Error for ColorParseError {}

fn from_hex(value: &str) -> Result<u8, ColorParseError> {
    if value.is_empty() || value.len() > 4 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ColorParseError);
    }
    let color = u32::from_str_radix(value, 16).map_err(|_| ColorParseError)?;
    let divisor: u32 = match value.len() {
        1 => 0xF,
        2 => 0xFF,
        3 => 0xFFF,
        _ => 0xFFFF,
    };
    Ok((color * 255 / divisor) as u8)
}

fn from_intensity(value: &str) -> Result<u8, ColorParseError> {
    let v: f64 = value.parse().map_err(|_| ColorParseError)?;
    if !(0.0..=1.0).contains(&v) {
        return Err(ColorParseError);
    }
    Ok((v * 255.0) as u8)
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Accepts `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb`, the same without
    /// `#`, `rgb:r/g/b`, `rgbi:r/g/b` and X11 color names.
    pub fn parse(value: &str) -> Result<Self, ColorParseError> {
        let input = value.trim_matches(|c| c == ' ' || c == '\t');
        if input.is_empty() {
            return Err(ColorParseError);
        }

        if let Some(hex) = input.strip_prefix('#') {
            return match hex.len() {
                3 | 6 | 9 | 12 => {
                    let n = hex.len() / 3;
                    if !hex.is_ascii() {
                        return Err(ColorParseError);
                    }
                    Ok(Self::new(
                        from_hex(&hex[..n])?,
                        from_hex(&hex[n..2 * n])?,
                        from_hex(&hex[2 * n..])?,
                    ))
                }
                _ => Err(ColorParseError),
            };
        }

        if let Some((_, c)) = X11_COLORS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(input))
        {
            return Ok(Self::new(c[0], c[1], c[2]));
        }

        if input.is_ascii() {
            match input.len() {
                3 => {
                    return Ok(Self::new(
                        from_hex(&input[0..1])?,
                        from_hex(&input[1..2])?,
                        from_hex(&input[2..3])?,
                    ))
                }
                6 => {
                    return Ok(Self::new(
                        from_hex(&input[0..2])?,
                        from_hex(&input[2..4])?,
                        from_hex(&input[4..6])?,
                    ))
                }
                _ => {}
            }
        }

        if input.len() < "rgb:a/a/a".len() || !input.starts_with("rgb") {
            return Err(ColorParseError);
        }
        let mut rest = &input[3..];
        let intensity = rest.starts_with('i');
        if intensity {
            rest = &rest[1..];
        }
        let rest = rest.strip_prefix(':').ok_or(ColorParseError)?;
        let mut parts = rest.splitn(3, '/');
        let (r, g, b) = match (parts.next(), parts.next(), parts.next()) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => return Err(ColorParseError),
        };
        let conv = if intensity { from_intensity } else { from_hex };
        Ok(Self::new(conv(r)?, conv(g)?, conv(b)?))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// A color that may also refer to the cell's own colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalColor {
    Color(Rgb),
    CellForeground,
    CellBackground,
}

impl TerminalColor {
    pub fn parse(value: &str) -> Result<Self, ColorParseError> {
        match value {
            "cell-foreground" => Ok(Self::CellForeground),
            "cell-background" => Ok(Self::CellBackground),
            _ => Rgb::parse(value).map(Self::Color),
        }
    }
}

/// Parses an `N=color` palette entry; N accepts decimal and 0x/0o/0b prefixes.
pub fn parse_palette_entry(value: &str) -> Result<(u8, Rgb), PaletteEntryError> {
    let (index, color) = value.split_once('=').ok_or(PaletteEntryError::InvalidFormat)?;
    let index = parse_int_auto(index.trim_matches(|c| c == ' ' || c == '\t'))?;
    let color = Rgb::parse(color).map_err(|_| PaletteEntryError::InvalidFormat)?;
    Ok((index, color))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteEntryError {
    InvalidFormat,
    Overflow,
}

fn parse_int_auto(text: &str) -> Result<u8, PaletteEntryError> {
    let (radix, digits) = match text.get(..2) {
        Some("0x") | Some("0X") => (16, &text[2..]),
        Some("0o") | Some("0O") => (8, &text[2..]),
        Some("0b") | Some("0B") => (2, &text[2..]),
        _ => (10, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(PaletteEntryError::InvalidFormat);
    }
    match u128::from_str_radix(&digits.replace('_', ""), radix) {
        Ok(v) if v <= u8::MAX as u128 => Ok(v as u8),
        Ok(_) => Err(PaletteEntryError::Overflow),
        Err(e) => match e.kind() {
            std::num::IntErrorKind::PosOverflow => Err(PaletteEntryError::Overflow),
            _ => Err(PaletteEntryError::InvalidFormat),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(Rgb::parse("#AABBCC"), Ok(Rgb::new(170, 187, 204)));
        assert_eq!(Rgb::parse("abc"), Ok(Rgb::new(170, 187, 204)));
        assert_eq!(Rgb::parse("#fff"), Ok(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::parse("#ffffffffffff"), Ok(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::parse("  #012345  "), Ok(Rgb::new(1, 35, 69)));
        assert!(Rgb::parse("#12345").is_err());
        assert!(Rgb::parse("").is_err());
    }

    #[test]
    fn parses_names_and_rgb_forms() {
        assert_eq!(Rgb::parse("black"), Ok(Rgb::new(0, 0, 0)));
        assert_eq!(Rgb::parse("White"), Ok(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::parse("rgb:f/f/f"), Ok(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::parse("rgb:ff/00/80"), Ok(Rgb::new(255, 0, 128)));
        assert_eq!(Rgb::parse("rgbi:1.0/0/0.5"), Ok(Rgb::new(255, 0, 127)));
        assert!(Rgb::parse("notacolor").is_err());
    }

    #[test]
    fn palette_entries() {
        assert_eq!(parse_palette_entry("0=#AABBCC"), Ok((0, Rgb::new(170, 187, 204))));
        assert_eq!(parse_palette_entry("0xF=#ABCDEF"), Ok((15, Rgb::new(171, 205, 239))));
        assert_eq!(parse_palette_entry("0b1=#014589"), Ok((1, Rgb::new(1, 69, 137))));
        assert_eq!(parse_palette_entry("0o7=#234567"), Ok((7, Rgb::new(35, 69, 103))));
        assert_eq!(parse_palette_entry(" 1= #DDEEFF    "), Ok((1, Rgb::new(221, 238, 255))));
        assert_eq!(parse_palette_entry("1=black"), Ok((1, Rgb::new(0, 0, 0))));
        assert_eq!(parse_palette_entry(" "), Err(PaletteEntryError::InvalidFormat));
        assert_eq!(parse_palette_entry("256=#AABBCC"), Err(PaletteEntryError::Overflow));
        assert_eq!(parse_palette_entry("1=notacolor"), Err(PaletteEntryError::InvalidFormat));
    }

    #[test]
    fn default_palette_layout() {
        let p = default_palette();
        assert_eq!(p[16], Rgb::new(0, 0, 0));
        assert_eq!(p[21], Rgb::new(0, 0, 255));
        assert_eq!(p[231], Rgb::new(255, 255, 255));
        assert_eq!(p[232], Rgb::new(8, 8, 8));
        assert_eq!(p[255], Rgb::new(238, 238, 238));
    }
}
