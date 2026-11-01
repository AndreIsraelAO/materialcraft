//! Parameter values: the typed data a node parameter, an exposed graph input or a function-graph
//! constant holds.

use serde::{Deserialize, Serialize};

/// One control point of a [`Curve`]: position and value, both 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

/// A monotone tone curve through sorted control points (linear when it has two points).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub points: Vec<CurvePoint>,
}

impl Default for Curve {
    fn default() -> Self {
        Curve { points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }] }
    }
}

impl Curve {
    /// Value at `x` (0..1): monotone cubic (Fritsch–Carlson) through the points, clamped ends.
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.points;
        let n = p.len();
        match n {
            0 => return x,
            1 => return p[0].y,
            _ => {}
        }
        if x <= p[0].x {
            return p[0].y;
        }
        if x >= p[n - 1].x {
            return p[n - 1].y;
        }
        let mut i = 0;
        while i + 2 < n && x > p[i + 1].x {
            i += 1;
        }
        let slope = |a: usize| {
            let (l, r) = (a.saturating_sub(1), (a + 1).min(n - 1));
            let dx = p[r].x - p[l].x;
            if dx.abs() < 1e-6 { 0.0 } else { (p[r].y - p[l].y) / dx }
        };
        let (a, b) = (p[i], p[i + 1]);
        let h = (b.x - a.x).max(1e-6);
        let d = (b.y - a.y) / h;
        let (mut m0, mut m1) = (slope(i), slope(i + 1));
        if d == 0.0 {
            m0 = 0.0;
            m1 = 0.0;
        } else {
            // Keep the segment monotone.
            if m0 * d < 0.0 {
                m0 = 0.0;
            }
            if m1 * d < 0.0 {
                m1 = 0.0;
            }
            let (al, be) = (m0 / d, m1 / d);
            let s = al * al + be * be;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m0 = t * al * d;
                m1 = t * be * d;
            }
        }
        let t = (x - a.x) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * a.y + (t3 - 2.0 * t2 + t) * h * m0 + (-2.0 * t3 + 3.0 * t2) * b.y + (t3 - t2) * h * m1
    }
}

/// One stop of a [`Gradient`]: position 0..1 and linear RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientKey {
    pub pos: f32,
    pub color: [f32; 4],
}

/// A color ramp (gradient map).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub keys: Vec<GradientKey>,
}

impl Default for Gradient {
    fn default() -> Self {
        Gradient { keys: vec![GradientKey { pos: 0.0, color: [0.0, 0.0, 0.0, 1.0] }, GradientKey { pos: 1.0, color: [1.0, 1.0, 1.0, 1.0] }] }
    }
}

impl Gradient {
    /// Linear interpolation between the sorted keys.
    pub fn eval(&self, t: f32) -> [f32; 4] {
        let k = &self.keys;
        let Some(first) = k.first() else { return [t, t, t, 1.0] };
        if t <= first.pos {
            return first.color;
        }
        for w in k.windows(2) {
            if t <= w[1].pos {
                let span = (w[1].pos - w[0].pos).max(1e-6);
                let f = ((t - w[0].pos) / span).clamp(0.0, 1.0);
                let (a, b) = (w[0].color, w[1].color);
                return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, a[3] + (b[3] - a[3]) * f];
            }
        }
        k.last().map_or([t, t, t, 1.0], |l| l.color)
    }

    /// Keys sorted by position (call after editing).
    pub fn sort(&mut self) {
        self.keys.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    }
}

/// A typed parameter value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "camelCase")]
pub enum Value {
    Float(f32),
    Float2([f32; 2]),
    Float3([f32; 3]),
    Float4([f32; 4]),
    Int(i32),
    Int2([i32; 2]),
    Bool(bool),
    /// Linear RGBA, 0..1.
    Color([f32; 4]),
    /// Index into the parameter's options.
    Enum(u32),
    Text(String),
    Curve(Curve),
    Gradient(Gradient),
    /// A function graph (pixel/value processors, dynamic parameters).
    Function(Box<crate::function::FnGraph>),
}

impl Value {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f32),
            Value::Enum(v) => Some(*v as f32),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Float2(v) => Some(v[0]),
            Value::Float3(v) => Some(v[0]),
            Value::Float4(v) | Value::Color(v) => Some(v[0]),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Enum(v) => i32::try_from(*v).ok(),
            Value::Float(v) if v.is_finite() => Some(v.round() as i32),
            Value::Bool(b) => Some(i32::from(*b)),
            Value::Int2(v) => Some(v[0]),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(v) => Some(*v != 0),
            Value::Float(v) => Some(*v != 0.0),
            _ => None,
        }
    }

    /// Up to four floats (missing ones are 0; a color keeps its alpha).
    pub fn as_vec4(&self) -> Option<[f32; 4]> {
        match self {
            Value::Float(v) => Some([*v, *v, *v, 1.0]),
            Value::Float2(v) => Some([v[0], v[1], 0.0, 0.0]),
            Value::Float3(v) => Some([v[0], v[1], v[2], 0.0]),
            Value::Float4(v) | Value::Color(v) => Some(*v),
            Value::Int(v) => Some([*v as f32; 4]),
            Value::Int2(v) => Some([v[0] as f32, v[1] as f32, 0.0, 0.0]),
            _ => None,
        }
    }

    /// Short type name, for errors and inspect output.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Float(_) => "float",
            Value::Float2(_) => "float2",
            Value::Float3(_) => "float3",
            Value::Float4(_) => "float4",
            Value::Int(_) => "int",
            Value::Int2(_) => "int2",
            Value::Bool(_) => "bool",
            Value::Color(_) => "color",
            Value::Enum(_) => "enum",
            Value::Text(_) => "text",
            Value::Curve(_) => "curve",
            Value::Gradient(_) => "gradient",
            Value::Function(_) => "function",
        }
    }

    /// Coerce a JSON value (from a command or MCP call) into the type of `self`.
    pub fn coerce_json(&self, j: &serde_json::Value) -> Option<Value> {
        use serde_json::Value as J;
        let f = |v: &J| v.as_f64().map(|x| x as f32).filter(|x| x.is_finite());
        let arr = |n: usize| -> Option<Vec<f32>> {
            match j {
                J::Array(a) if a.len() >= n => a.iter().take(n).map(f).collect(),
                J::Number(_) => f(j).map(|v| vec![v; n]),
                _ => None,
            }
        };
        // A typed value as the API writes it ({"t": .., "v": ..}).
        if let Ok(v) = serde_json::from_value::<Value>(j.clone()) {
            if v.type_name() == self.type_name() {
                return Some(v);
            }
            // Another type (a float input bound to an int parameter): coerce its payload.
            return j.get("v").and_then(|p| self.coerce_json(p));
        }
        Some(match self {
            Value::Float(_) => Value::Float(f(j)?),
            Value::Float2(_) => {
                let a = arr(2)?;
                Value::Float2([a[0], a[1]])
            }
            Value::Float3(_) => {
                let a = arr(3)?;
                Value::Float3([a[0], a[1], a[2]])
            }
            Value::Float4(_) => {
                let a = arr(4)?;
                Value::Float4([a[0], a[1], a[2], a[3]])
            }
            Value::Color(_) => Value::Color(parse_color(j)?),
            Value::Int(_) => Value::Int(f(j)?.round() as i32),
            Value::Int2(_) => {
                let a = arr(2)?;
                Value::Int2([a[0].round() as i32, a[1].round() as i32])
            }
            Value::Bool(_) => Value::Bool(j.as_bool().or_else(|| f(j).map(|v| v != 0.0))?),
            Value::Enum(_) => Value::Enum(u32::try_from(j.as_u64()?).ok()?),
            Value::Text(_) => Value::Text(j.as_str()?.to_string()),
            Value::Curve(_) => {
                let pts: Vec<[f32; 2]> = serde_json::from_value(j.clone()).ok()?;
                let mut points: Vec<CurvePoint> =
                    pts.into_iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| CurvePoint { x: p[0], y: p[1] }).collect();
                points.sort_by(|a, b| a.x.total_cmp(&b.x));
                Value::Curve(Curve { points })
            }
            Value::Gradient(_) => {
                let keys: Vec<(f32, serde_json::Value)> = serde_json::from_value(j.clone()).ok()?;
                let mut g = Gradient { keys: keys.iter().filter_map(|(p, c)| Some(GradientKey { pos: *p, color: parse_color(c)? })).collect() };
                g.sort();
                Value::Gradient(g)
            }
            Value::Function(_) => return None,
        })
    }
}

/// `[r, g, b(, a)]` linear floats, a single gray number, or `"#rrggbb(aa)"` (sRGB, converted to linear).
pub fn parse_color(j: &serde_json::Value) -> Option<[f32; 4]> {
    use serde_json::Value as J;
    match j {
        J::Array(a) if a.len() >= 3 => {
            let v: Vec<f32> = a.iter().take(4).filter_map(|x| x.as_f64().map(|x| x as f32)).filter(|x| x.is_finite()).collect();
            if v.len() < 3 {
                return None;
            }
            Some([v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)])
        }
        J::Number(n) => n.as_f64().map(|v| v as f32).filter(|v| v.is_finite()).map(|v| [v, v, v, 1.0]),
        J::String(s) => {
            let s = s.trim_start_matches('#');
            if s.len() != 6 && s.len() != 8 {
                return None;
            }
            let c = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok().map(|v| v as f32 / 255.0);
            let a = if s.len() == 8 { c(6)? } else { 1.0 };
            Some([srgb_to_linear(c(0)?), srgb_to_linear(c(2)?), srgb_to_linear(c(4)?), a])
        }
        _ => None,
    }
}

/// sRGB transfer function, encoded → linear.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// sRGB transfer function, linear → encoded.
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_identity_and_monotone() {
        let c = Curve::default();
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            assert!((c.eval(x) - x).abs() < 1e-4);
        }
        let s =
            Curve { points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 0.3, y: 0.1 }, CurvePoint { x: 0.7, y: 0.9 }, CurvePoint { x: 1.0, y: 1.0 }] };
        let mut last = -1.0;
        for i in 0..=100 {
            let v = s.eval(i as f32 / 100.0);
            assert!(v >= last - 1e-5);
            last = v;
        }
    }

    #[test]
    fn coerce_from_json() {
        assert_eq!(Value::Float(0.0).coerce_json(&serde_json::json!(0.5)), Some(Value::Float(0.5)));
        assert_eq!(Value::Color([0.0; 4]).coerce_json(&serde_json::json!("#ffffff")), Some(Value::Color([1.0, 1.0, 1.0, 1.0])));
        assert_eq!(Value::Float2([0.0; 2]).coerce_json(&serde_json::json!([1, 2])), Some(Value::Float2([1.0, 2.0])));
        assert_eq!(Value::Float(0.0).coerce_json(&serde_json::json!("x")), None);
        assert_eq!(Value::Float(0.0).coerce_json(&serde_json::json!(f64::MAX)), None);
        // A typed value of another type converts through its payload.
        assert_eq!(Value::Int(0).coerce_json(&serde_json::json!({"t": "float", "v": 8.6})), Some(Value::Int(9)));
        assert_eq!(Value::Int(0).coerce_json(&serde_json::json!({"t": "text", "v": "x"})), None);
    }

    #[test]
    fn gradient_interpolates() {
        let g = Gradient::default();
        assert_eq!(g.eval(0.5), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(Gradient { keys: vec![] }.eval(0.25)[0], 0.25);
    }
}
