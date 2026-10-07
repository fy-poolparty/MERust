//! UE3 tagged properties (FPropertyTag, ArVer 536: 4-byte bool values, no byte enum names).

use crate::package::{FName, Package};
use crate::reader::{Reader, Result};

/// `RF_HasStack`: the object starts with an FStateFrame before its properties.
pub const RF_HAS_STACK: u64 = 0x0200_0000_0000_0000;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    Byte(u8),
    Name(String),
    Object(i32),
    Str(String),
    /// Struct or array payload, interpreted by the caller (layouts depend on the struct/inner type).
    Raw,
}

#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub ty: String,
    pub struct_name: String,
    pub array_index: i32,
    /// Byte range of the value inside the buffer passed to `read_props`.
    pub start: usize,
    pub size: usize,
    pub value: Value,
}

impl Prop {
    pub fn bytes<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        &data[self.start..self.start + self.size]
    }

    pub fn as_vec3(&self, data: &[u8]) -> Option<[f32; 3]> {
        (self.size >= 12).then(|| Reader::at(data, self.start).vec3().ok()).flatten()
    }

    /// Unreal rotator (Pitch, Yaw, Roll ints, 65536 = 360 degrees).
    pub fn as_rotator(&self, data: &[u8]) -> Option<[i32; 3]> {
        let mut r = Reader::at(data, self.start);
        (self.size >= 12).then(|| Some([r.i32().ok()?, r.i32().ok()?, r.i32().ok()?])).flatten()
    }

    /// Array of 4-byte elements (object refs, ints, floats).
    pub fn as_i32_array(&self, data: &[u8]) -> Vec<i32> {
        let mut r = Reader::at(data, self.start);
        let Ok(n) = r.count(1 << 20) else { return Vec::new() };
        if 4 + n * 4 != self.size {
            return Vec::new();
        }
        (0..n).filter_map(|_| r.i32().ok()).collect()
    }
}

fn fname(r: &mut Reader) -> Result<FName> {
    Ok(FName { index: r.i32()?, number: r.i32()? })
}

/// Skip the object header (state frame, net index) and return where tagged properties start.
pub fn props_start(pkg: &Package, export: usize) -> Result<usize> {
    let e = &pkg.exports[export];
    let data = pkg.export_bytes(export);
    let mut r = Reader::new(data);
    if e.flags & RF_HAS_STACK != 0 {
        // FStateFrame: Node, StateNode, ProbeMask (qword), LatentAction (int), StateStack, Offset.
        let node = r.i32()?;
        let _state_node = r.i32()?;
        r.u64()?;
        r.i32()?;
        let stack = r.count(1000)?;
        r.skip(stack * 12)?;
        if node != 0 {
            r.i32()?;
        }
    }
    // UComponent prepends TemplateOwnerClass (and TemplateName for templates) before the
    // UObject NetIndex; other objects have just the NetIndex. Pick the layout whose first
    // tag reads as a real property.
    let base = r.pos;
    for skip in [4usize, 8, 16] {
        if looks_like_tag(pkg, data, base + skip) {
            return Ok(base + skip);
        }
    }
    Ok(base + 4)
}

fn looks_like_tag(pkg: &Package, data: &[u8], at: usize) -> bool {
    let mut r = Reader::at(data, at);
    let Ok(name) = fname(&mut r) else { return false };
    if name.index < 0 || name.index as usize >= pkg.names.len() {
        return false;
    }
    if pkg.name_str(name) == "None" {
        return true;
    }
    let Ok(ty) = fname(&mut r) else { return false };
    ty.index >= 0 && (ty.index as usize) < pkg.names.len() && pkg.name_str(ty).ends_with("Property")
}

/// Read tagged properties from `data` starting at `start` until "None".
/// Returns the properties and the offset just past the terminating "None".
pub fn read_props(pkg: &Package, data: &[u8], start: usize) -> Result<(Vec<Prop>, usize)> {
    let mut r = Reader::at(data, start);
    let mut props = Vec::new();
    loop {
        let name = fname(&mut r)?;
        let name_s = pkg.name(name);
        if name_s == "None" {
            return Ok((props, r.pos));
        }
        let ty = pkg.name(fname(&mut r)?);
        let size = r.i32()?;
        if !(0..=(1 << 26)).contains(&size) {
            return r.err(format!("property {name_s} has size {size}"));
        }
        let size = size as usize;
        let array_index = r.i32()?;
        let mut struct_name = String::new();
        let mut value = Value::Raw;
        match ty.as_str() {
            "StructProperty" => struct_name = pkg.name(fname(&mut r)?),
            "BoolProperty" => value = Value::Bool(r.i32()? != 0),
            _ => {}
        }
        let vstart = r.pos;
        let mut v = Reader::at(data, vstart);
        match ty.as_str() {
            "IntProperty" if size == 4 => value = Value::Int(v.i32()?),
            "FloatProperty" if size == 4 => value = Value::Float(v.f32()?),
            "ByteProperty" if size == 1 => value = Value::Byte(v.u8()?),
            // Mirror's Edge stores enum-typed bytes as the enumerator's name.
            "ByteProperty" if size == 8 => value = Value::Name(pkg.name(fname(&mut v)?)),
            "NameProperty" if size == 8 => value = Value::Name(pkg.name(fname(&mut v)?)),
            "ObjectProperty" | "ComponentProperty" | "ClassProperty" | "InterfaceProperty" if size == 4 => {
                value = Value::Object(v.i32()?)
            }
            "StrProperty" => value = Value::Str(v.fstring()?),
            _ => {}
        }
        r.skip(size)?;
        props.push(Prop { name: name_s, ty, struct_name, array_index, start: vstart, size, value });
    }
}

/// Convenience: properties of an export, with offsets relative to `pkg.export_bytes(export)`.
pub fn export_props(pkg: &Package, export: usize) -> Result<(Vec<Prop>, usize)> {
    let start = props_start(pkg, export)?;
    read_props(pkg, pkg.export_bytes(export), start)
}

/// An `ArrayProperty` of non-native structs: count, then each element as tagged properties.
/// Offsets in the returned props are relative to `data`.
pub fn struct_array(pkg: &Package, data: &[u8], array: &Prop) -> Vec<Vec<Prop>> {
    let mut r = Reader::at(data, array.start);
    let Ok(n) = r.count(1 << 16) else { return Vec::new() };
    let mut pos = r.pos;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        match read_props(pkg, data, pos) {
            Ok((props, end)) => {
                out.push(props);
                pos = end;
            }
            Err(_) => break,
        }
    }
    out
}

pub fn find<'a>(props: &'a [Prop], name: &str) -> Option<&'a Prop> {
    props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}
