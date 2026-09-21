use std::collections::{BTreeMap, BTreeSet};

use crate::XAtom;

pub const X_ATOM_MAX_NAME_LEN: usize = 256;
pub const X_ATOM_LAST_PREDEFINED: XAtom = 68;

pub const X_ATOM_PRIMARY: XAtom = 1;
pub const X_ATOM_ATOM: XAtom = 4;
pub const X_ATOM_CARDINAL: XAtom = 6;
pub const X_ATOM_RESOURCE_MANAGER: XAtom = 23;
pub const X_ATOM_STRING: XAtom = 31;
pub const X_ATOM_WM_NAME: XAtom = 39;
pub const X_ATOM_WM_CLASS: XAtom = 67;
pub const X_ATOM_WINDOW: XAtom = 33;

pub const X_ATOM_NAME_PRIMARY: &str = "PRIMARY";
pub const X_ATOM_NAME_ATOM: &str = "ATOM";
pub const X_ATOM_NAME_RESOURCE_MANAGER: &str = "RESOURCE_MANAGER";
pub const X_ATOM_NAME_STRING: &str = "STRING";
pub const X_ATOM_NAME_WM_NAME: &str = "WM_NAME";
pub const X_ATOM_NAME_WM_CLASS: &str = "WM_CLASS";
pub const X_ATOM_NAME_NET_WM_NAME: &str = "_NET_WM_NAME";
pub const X_ATOM_NAME_NET_SUPPORTED: &str = "_NET_SUPPORTED";
pub const X_ATOM_NAME_NET_SUPPORTING_WM_CHECK: &str = "_NET_SUPPORTING_WM_CHECK";
pub const X_ATOM_NAME_NET_ACTIVE_WINDOW: &str = "_NET_ACTIVE_WINDOW";
pub const X_ATOM_NAME_NET_WM_WINDOW_TYPE: &str = "_NET_WM_WINDOW_TYPE";
pub const X_ATOM_NAME_NET_WM_STATE: &str = "_NET_WM_STATE";
pub const X_ATOM_NAME_NET_WM_STATE_FULLSCREEN: &str = "_NET_WM_STATE_FULLSCREEN";
pub const X_ATOM_NAME_NET_WM_STATE_HIDDEN: &str = "_NET_WM_STATE_HIDDEN";
pub const X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_HORZ: &str = "_NET_WM_STATE_MAXIMIZED_HORZ";
pub const X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_VERT: &str = "_NET_WM_STATE_MAXIMIZED_VERT";
pub const X_ATOM_NAME_NET_WM_STRUT: &str = "_NET_WM_STRUT";
pub const X_ATOM_NAME_NET_WM_STRUT_PARTIAL: &str = "_NET_WM_STRUT_PARTIAL";
pub const X_ATOM_NAME_WM_STATE: &str = "WM_STATE";
pub const X_ATOM_NAME_WM_PROTOCOLS: &str = "WM_PROTOCOLS";
pub const X_ATOM_NAME_UTF8_STRING: &str = "UTF8_STRING";
pub const X_ATOM_NAME_WM_DELETE_WINDOW: &str = "WM_DELETE_WINDOW";
pub const X_ATOM_NAME_RANDR_EDID: &str = "EDID";
pub const X_ATOM_NAME_RANDR_NON_DESKTOP: &str = "non-desktop";

const X_RANDR_CONVENTIONAL_OUTPUT_PROPERTY_NAMES: &[&str] = &[
    "Backlight",
    X_ATOM_NAME_RANDR_EDID,
    "SignalFormat",
    "SignalProperties",
    "ConnectorType",
    "ConnectorNumber",
    "CompatibilityList",
    "CloneList",
    "Border",
    "BorderDimensions",
    "GUID",
    "TILE",
    X_ATOM_NAME_RANDR_NON_DESKTOP,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAtomError {
    EmptyName,
    NameTooLong { len: usize, max: usize },
    InvalidName,
    AtomSpaceExhausted,
}

impl core::fmt::Display for XAtomError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for XAtomError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAtomTable {
    names_by_atom: BTreeMap<XAtom, String>,
    atoms_by_name: BTreeMap<String, XAtom>,
    next_dynamic: XAtom,
    /// The atoms this server defines itself, which no client created and
    /// which therefore survive every client leaving.
    fixed: BTreeSet<XAtom>,
}

impl Default for XAtomTable {
    fn default() -> Self {
        Self::new()
    }
}

impl XAtomTable {
    pub fn new() -> Self {
        let mut table = Self {
            names_by_atom: BTreeMap::new(),
            atoms_by_name: BTreeMap::new(),
            next_dynamic: X_ATOM_LAST_PREDEFINED + 1,
            fixed: BTreeSet::new(),
        };
        for (atom, name) in X_PREDEFINED_ATOMS {
            table.insert_predefined(*atom, name);
        }
        for name in X_RANDR_CONVENTIONAL_OUTPUT_PROPERTY_NAMES {
            table
                .intern(name, false)
                .expect("fixed RandR output property atom is valid")
                .expect("interning a fixed RandR output property returns an atom");
        }
        table.fixed = table.names_by_atom.keys().copied().collect();
        table
    }

    pub fn intern(
        &mut self,
        name: impl AsRef<str>,
        only_if_exists: bool,
    ) -> Result<Option<XAtom>, XAtomError> {
        let name = validated_atom_name(name.as_ref())?;
        if let Some(atom) = self.atoms_by_name.get(name) {
            return Ok(Some(*atom));
        }
        if only_if_exists {
            return Ok(None);
        }

        let atom = self.next_dynamic;
        self.next_dynamic = self
            .next_dynamic
            .checked_add(1)
            .ok_or(XAtomError::AtomSpaceExhausted)?;
        self.names_by_atom.insert(atom, name.to_owned());
        self.atoms_by_name.insert(name.to_owned(), atom);
        Ok(Some(atom))
    }

    /// Forgets every atom a client interned, and reports which they were.
    ///
    /// The protocol scopes an atom to the server's lifetime: "when the last
    /// connection closes, atoms created by a call to XInternAtom become
    /// undefined". Our authority outlives its connections, so the moment the
    /// last one goes is the moment that has to be honoured explicitly.
    ///
    /// Predefined atoms and the fixed RandR property names are not client
    /// creations and stay. `next_dynamic` deliberately does not rewind: a
    /// freed atom id is never reissued, because a later client interning an
    /// unrelated name would otherwise inherit whatever the old id still owns.
    /// The returned list is what the caller must clear alongside.
    pub fn forget_client_interned(&mut self) -> Vec<XAtom> {
        let fixed: BTreeSet<XAtom> = self.fixed.iter().copied().collect();
        let removed: Vec<XAtom> = self
            .names_by_atom
            .keys()
            .copied()
            .filter(|atom| !fixed.contains(atom))
            .collect();
        for atom in &removed {
            if let Some(name) = self.names_by_atom.remove(atom) {
                self.atoms_by_name.remove(&name);
            }
        }
        removed
    }

    pub fn name(&self, atom: XAtom) -> Option<&str> {
        self.names_by_atom.get(&atom).map(String::as_str)
    }

    pub fn atom(&self, name: &str) -> Option<XAtom> {
        self.atoms_by_name.get(name).copied()
    }

    pub fn is_metadata_candidate_atom(&self, atom: XAtom) -> bool {
        self.name(atom).is_some_and(is_metadata_candidate_name)
    }

    fn insert_predefined(&mut self, atom: XAtom, name: &str) {
        self.names_by_atom.insert(atom, name.to_owned());
        self.atoms_by_name.insert(name.to_owned(), atom);
    }
}

const X_PREDEFINED_ATOMS: &[(XAtom, &str)] = &[
    (X_ATOM_PRIMARY, X_ATOM_NAME_PRIMARY),
    (2, "SECONDARY"),
    (3, "ARC"),
    (X_ATOM_ATOM, X_ATOM_NAME_ATOM),
    (5, "BITMAP"),
    (6, "CARDINAL"),
    (7, "COLORMAP"),
    (8, "CURSOR"),
    (9, "CUT_BUFFER0"),
    (10, "CUT_BUFFER1"),
    (11, "CUT_BUFFER2"),
    (12, "CUT_BUFFER3"),
    (13, "CUT_BUFFER4"),
    (14, "CUT_BUFFER5"),
    (15, "CUT_BUFFER6"),
    (16, "CUT_BUFFER7"),
    (17, "DRAWABLE"),
    (18, "FONT"),
    (19, "INTEGER"),
    (20, "PIXMAP"),
    (21, "POINT"),
    (22, "RECTANGLE"),
    (X_ATOM_RESOURCE_MANAGER, X_ATOM_NAME_RESOURCE_MANAGER),
    (24, "RGB_COLOR_MAP"),
    (25, "RGB_BEST_MAP"),
    (26, "RGB_BLUE_MAP"),
    (27, "RGB_DEFAULT_MAP"),
    (28, "RGB_GRAY_MAP"),
    (29, "RGB_GREEN_MAP"),
    (30, "RGB_RED_MAP"),
    (X_ATOM_STRING, X_ATOM_NAME_STRING),
    (32, "VISUALID"),
    (33, "WINDOW"),
    (34, "WM_COMMAND"),
    (35, "WM_HINTS"),
    (36, "WM_CLIENT_MACHINE"),
    (37, "WM_ICON_NAME"),
    (38, "WM_ICON_SIZE"),
    (X_ATOM_WM_NAME, X_ATOM_NAME_WM_NAME),
    (40, "WM_NORMAL_HINTS"),
    (41, "WM_SIZE_HINTS"),
    (42, "WM_ZOOM_HINTS"),
    (43, "MIN_SPACE"),
    (44, "NORM_SPACE"),
    (45, "MAX_SPACE"),
    (46, "END_SPACE"),
    (47, "SUPERSCRIPT_X"),
    (48, "SUPERSCRIPT_Y"),
    (49, "SUBSCRIPT_X"),
    (50, "SUBSCRIPT_Y"),
    (51, "UNDERLINE_POSITION"),
    (52, "UNDERLINE_THICKNESS"),
    (53, "STRIKEOUT_ASCENT"),
    (54, "STRIKEOUT_DESCENT"),
    (55, "ITALIC_ANGLE"),
    (56, "X_HEIGHT"),
    (57, "QUAD_WIDTH"),
    (58, "WEIGHT"),
    (59, "POINT_SIZE"),
    (60, "RESOLUTION"),
    (61, "COPYRIGHT"),
    (62, "NOTICE"),
    (63, "FONT_NAME"),
    (64, "FAMILY_NAME"),
    (65, "FULL_NAME"),
    (66, "CAP_HEIGHT"),
    (X_ATOM_WM_CLASS, X_ATOM_NAME_WM_CLASS),
    (68, "WM_TRANSIENT_FOR"),
];

pub fn is_metadata_candidate_name(name: &str) -> bool {
    matches!(
        name,
        X_ATOM_NAME_WM_CLASS
            | X_ATOM_NAME_WM_NAME
            | X_ATOM_NAME_NET_WM_NAME
            | X_ATOM_NAME_WM_PROTOCOLS
    )
}

fn validated_atom_name(name: &str) -> Result<&str, XAtomError> {
    if name.is_empty() {
        return Err(XAtomError::EmptyName);
    }
    if name.len() > X_ATOM_MAX_NAME_LEN {
        return Err(XAtomError::NameTooLong {
            len: name.len(),
            max: X_ATOM_MAX_NAME_LEN,
        });
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    {
        return Err(XAtomError::InvalidName);
    }
    Ok(name)
}
