//! Validation and evaluation of a parsed program into a [`SceneGraph`].
//!
//! The evaluator is the only place that knows about scopes, fuel and limits.
//! It never panics on user input; every failure is a [`Diagnostic`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use scene_syntax::ast::{
    Ambient, Anchor, Animation, AssignOp, Axis, Background, BinaryOp, Block, ElseBranch, Expr,
    ExprKind, Include, ItemKind, Material, Object, Origin, Program, Shape, Statement,
    StatementKind, Sun, UnaryOp,
};
use scene_syntax::{Diagnostic, Span, parse};

use crate::geometry::{
    Bounds, CompiledModel, CompiledObject, Instance, Part, Primitive, SceneAnimation, SceneGraph,
};
use crate::{
    DEFAULT_FUEL, MAX_ANIMATION_FRAMES, MAX_CANVAS_BLOCKS, MAX_COORD, MAX_INCLUDE_DEPTH,
    MAX_INCLUDED_BYTES, MAX_INSTANCES, MAX_LOOP_BOUND, MAX_OBJECTS, MAX_SCALE, MAX_SHAPES,
};

const BUILTIN_MATERIALS: [&str; 7] = ["wood", "metal", "light", "glass", "dark", "red", "white"];

/// Compile a `.model` program.
///
/// # Errors
///
/// Returns the first semantic [`Diagnostic`].
pub fn compile_model(program: &Program, base_dir: &Path) -> Result<CompiledModel, Diagnostic> {
    Evaluator::new(DEFAULT_FUEL).model(program, base_dir)
}

/// Compile a `.scene` program.
///
/// `base_dir` is used to resolve `include` paths; `root` is the sandbox root.
/// Both are usually the directory containing the scene file.
///
/// # Errors
///
/// Returns the first semantic or I/O [`Diagnostic`].
pub fn compile_scene(
    program: &Program,
    base_dir: &Path,
    root: &Path,
) -> Result<SceneGraph, Diagnostic> {
    compile_scene_with_fuel(program, base_dir, root, DEFAULT_FUEL)
}

/// Compile a `.scene` program with an explicit fuel budget.
///
/// # Errors
///
/// Returns the first semantic or I/O [`Diagnostic`].
pub fn compile_scene_with_fuel(
    program: &Program,
    base_dir: &Path,
    root: &Path,
    fuel: u64,
) -> Result<SceneGraph, Diagnostic> {
    Evaluator::new(fuel).scene(program, base_dir, root)
}

/// Compile frame `frame` of the animation `animation`: the scene is evaluated
/// with that animation's progress variable at `frame / (frames - 1)` and every
/// other animation's at 0.
///
/// # Errors
///
/// Returns the diagnostics of [`compile_scene`], `E0301` for an animation the
/// scene does not declare and `E0304` for a frame past its last one.
pub fn compile_scene_frame(
    program: &Program,
    base_dir: &Path,
    root: &Path,
    animation: &str,
    frame: u32,
) -> Result<SceneGraph, Diagnostic> {
    let mut evaluator = Evaluator::new(DEFAULT_FUEL);
    evaluator.frame = Some((animation.to_owned(), frame));
    let scene = evaluator.scene(program, base_dir, root)?;
    let Some(declared) = scene.animations.iter().find(|a| a.name == animation) else {
        return Err(diag(
            "E0301",
            format!("unknown animation `{animation}`"),
            program.span,
        ));
    };
    if frame >= declared.frames {
        return Err(diag(
            "E0304",
            format!(
                "animation `{animation}` has {} frames; frame {frame} does not exist",
                declared.frames
            ),
            program.span,
        ));
    }
    Ok(scene)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Number(f64),
    Bool(bool),
}

impl Value {
    fn type_name(self) -> &'static str {
        match self {
            Self::Number(_) => "number",
            Self::Bool(_) => "bool",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Variable {
    value: Value,
    mutable: bool,
}

#[derive(Default)]
struct Scopes {
    frames: Vec<BTreeMap<String, Variable>>,
}

impl Scopes {
    fn new() -> Self {
        Self {
            frames: vec![BTreeMap::new()],
        }
    }

    fn push(&mut self) {
        self.frames.push(BTreeMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn declare(
        &mut self,
        name: &str,
        value: Value,
        mutable: bool,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self.frames.iter().any(|frame| frame.contains_key(name)) {
            return Err(diag("E0303", format!("`{name}` is already declared"), span));
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name.to_owned(), Variable { value, mutable });
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Variable> {
        self.frames.iter().rev().find_map(|frame| frame.get(name))
    }

    fn assign(&mut self, name: &str, value: Value, span: Span) -> Result<(), Diagnostic> {
        for frame in self.frames.iter_mut().rev() {
            if let Some(variable) = frame.get_mut(name) {
                if !variable.mutable {
                    return Err(diag(
                        "E0304",
                        format!("cannot assign to immutable `{name}`"),
                        span,
                    ));
                }
                if std::mem::discriminant(&variable.value) != std::mem::discriminant(&value) {
                    return Err(diag(
                        "E0302",
                        format!(
                            "`{name}` is {}, not {}",
                            variable.value.type_name(),
                            value.type_name()
                        ),
                        span,
                    ));
                }
                variable.value = value;
                return Ok(());
            }
        }
        Err(diag("E0301", format!("unknown variable `{name}`"), span))
    }
}

struct Evaluator {
    fuel: u64,
    shapes: usize,
    merge_groups: u32,
    instances: usize,
    objects: BTreeMap<String, Arc<CompiledObject>>,
    materials: BTreeMap<String, PathBuf>,
    include_stack: Vec<PathBuf>,
    included_bytes: usize,
    /// The animation and frame being compiled, if any.
    frame: Option<(String, u32)>,
}

impl Evaluator {
    fn new(fuel: u64) -> Self {
        Self {
            fuel,
            shapes: 0,
            merge_groups: 0,
            instances: 0,
            objects: BTreeMap::new(),
            materials: BTreeMap::new(),
            include_stack: Vec::new(),
            included_bytes: 0,
            frame: None,
        }
    }

    fn spend(&mut self, span: Span) -> Result<(), Diagnostic> {
        if self.fuel == 0 {
            return Err(diag(
                "E0401",
                "evaluation budget exhausted; simplify the scene or reduce loops",
                span,
            ));
        }
        self.fuel -= 1;
        Ok(())
    }

    fn model(&mut self, program: &Program, base_dir: &Path) -> Result<CompiledModel, Diagnostic> {
        for item in &program.items {
            if let ItemKind::Material(material) = &item.kind {
                self.add_material(material, item.span, base_dir)?;
            }
        }
        for item in &program.items {
            match &item.kind {
                ItemKind::Material(_) => {}
                ItemKind::Object(object) => self.add_object(object, item.span)?,
                _ => {
                    return Err(diag(
                        "E0302",
                        "a .model file may contain only material and object declarations",
                        item.span,
                    ));
                }
            }
        }
        let bounds = self
            .objects
            .values()
            .fold(Bounds::empty(), |bounds, object| {
                bounds.union(object.local_bounds())
            })
            .or_zero();
        Ok(CompiledModel {
            materials: self.materials.clone(),
            objects: self.objects.clone(),
            bounds,
        })
    }

    fn scene(
        &mut self,
        program: &Program,
        base_dir: &Path,
        root: &Path,
    ) -> Result<SceneGraph, Diagnostic> {
        for item in &program.items {
            if let ItemKind::Material(material) = &item.kind {
                self.add_material(material, item.span, base_dir)?;
            }
        }

        let mut scopes = Scopes::new();
        let mut instances = Vec::new();
        let mut render = None;
        let mut scale = None;
        let mut canvas = None;
        let mut sun = [135.0, 45.0];
        let mut ambient = 0.35;
        let mut background = Background::Transparent;
        let mut animations: Vec<SceneAnimation> = Vec::new();

        for item in &program.items {
            match &item.kind {
                ItemKind::Material(_) => {}
                ItemKind::Include(include) => {
                    self.load_include(include, base_dir, root, item.span)?;
                }
                ItemKind::Object(object) => self.add_object(object, item.span)?,
                ItemKind::Render(value) => {
                    set_once(&mut render, value.kind, item.span, "render")?;
                }
                ItemKind::Scale(value) => {
                    let pixels = value.pixels_per_block;
                    if !(1..=MAX_SCALE).contains(&pixels) {
                        return Err(diag(
                            "E0304",
                            format!("scale must be in 1..={MAX_SCALE}"),
                            item.span,
                        ));
                    }
                    set_once(&mut scale, pixels, item.span, "scale")?;
                }
                ItemKind::Canvas(value) => {
                    let size = self.constant_tuple3(&value.size, item.span, "canvas")?;
                    for value in size {
                        check_positive(value, item.span, "canvas size")?;
                        if value > MAX_CANVAS_BLOCKS {
                            return Err(diag(
                                "E0402",
                                format!("canvas side must not exceed {MAX_CANVAS_BLOCKS} blocks"),
                                item.span,
                            ));
                        }
                    }
                    set_once(&mut canvas, size, item.span, "canvas")?;
                }
                ItemKind::Sun(value) => {
                    sun = self.sun(value, item.span)?;
                }
                ItemKind::Ambient(value) => {
                    ambient = self.ambient(value, item.span)?;
                }
                ItemKind::Background(value) => background = *value,
                ItemKind::Animation(animation) => {
                    let declared = Self::animation(animation, &animations)?;
                    let progress = match &self.frame {
                        Some((name, frame)) if *name == declared.name => declared.progress(*frame),
                        _ => 0.0,
                    };
                    scopes.declare(
                        &animation.variable,
                        Value::Number(progress),
                        false,
                        animation.variable_span,
                    )?;
                    animations.push(declared);
                }
                ItemKind::Statement(statement) => {
                    self.scene_statement(statement, &mut scopes, &mut instances)?;
                }
            }
        }

        let Some(render) = render else {
            return Err(diag("E0311", "scene requires `render`", program.span));
        };
        let Some(scale) = scale else {
            return Err(diag("E0311", "scene requires `scale`", program.span));
        };

        let bounds = instances
            .iter()
            .fold(Bounds::empty(), |bounds, instance| {
                bounds.union(instance.world_bounds())
            })
            .or_zero();

        Ok(SceneGraph {
            render,
            scale,
            canvas,
            sun,
            ambient,
            background,
            materials: self.materials.clone(),
            instances,
            bounds,
            animations,
        })
    }

    fn animation(
        animation: &Animation,
        declared: &[SceneAnimation],
    ) -> Result<SceneAnimation, Diagnostic> {
        if declared.iter().any(|other| other.name == animation.name) {
            return Err(diag(
                "E0303",
                format!("animation `{}` is declared twice", animation.name),
                animation.name_span,
            ));
        }
        if !(1..=MAX_ANIMATION_FRAMES).contains(&animation.frames) {
            return Err(diag(
                "E0304",
                format!("frames must be in 1..={MAX_ANIMATION_FRAMES}"),
                animation.frames_span,
            ));
        }
        if animation.duration == 0 {
            return Err(diag(
                "E0304",
                "frame duration must be at least 1 ms",
                animation.duration_span,
            ));
        }
        Ok(SceneAnimation {
            name: animation.name.clone(),
            frames: animation.frames,
            duration: animation.duration,
            looped: animation.looped,
        })
    }

    fn sun(&self, sun: &Sun, span: Span) -> Result<[f64; 2], Diagnostic> {
        let azimuth = self.constant_number(&sun.azimuth, span, "sun azimuth")?;
        let altitude = self.constant_number(&sun.altitude, span, "sun altitude")?;
        if !(0.0..=360.0).contains(&azimuth) || !(0.0..=90.0).contains(&altitude) {
            return Err(diag(
                "E0304",
                "sun expects azimuth 0..=360 and altitude 0..=90",
                span,
            ));
        }
        Ok([azimuth, altitude])
    }

    fn ambient(&self, ambient: &Ambient, span: Span) -> Result<f64, Diagnostic> {
        let value = self.constant_number(&ambient.value, span, "ambient")?;
        if !(0.0..=1.0).contains(&value) {
            return Err(diag("E0304", "ambient must be in 0..=1", span));
        }
        Ok(value)
    }

    fn add_material(
        &mut self,
        material: &Material,
        span: Span,
        base_dir: &Path,
    ) -> Result<(), Diagnostic> {
        if is_builtin(&material.name) {
            return Err(diag(
                "E0303",
                format!("material `{}` collides with a built-in name", material.name),
                span,
            ));
        }
        if !safe_relative(&material.path) {
            return Err(diag(
                "E0502",
                format!(
                    "material path `{}` must be relative and stay inside the project",
                    material.path
                ),
                span,
            ));
        }
        let path = base_dir.join(&material.path);
        if let Some(existing) = self.materials.get(&material.name) {
            if existing == &path {
                return Ok(());
            }
            return Err(diag(
                "E0303",
                format!("material `{}` is already defined", material.name),
                span,
            ));
        }
        self.materials.insert(material.name.clone(), path);
        Ok(())
    }

    fn add_object(&mut self, object: &Object, span: Span) -> Result<(), Diagnostic> {
        if self.objects.contains_key(&object.name) {
            return Err(diag(
                "E0303",
                format!("object `{}` is already defined", object.name),
                span,
            ));
        }
        if self.objects.len() >= MAX_OBJECTS {
            return Err(diag(
                "E0402",
                format!("more than {MAX_OBJECTS} objects"),
                span,
            ));
        }
        let compiled = self.compile_object(object, span)?;
        self.objects.insert(object.name.clone(), Arc::new(compiled));
        Ok(())
    }

    fn compile_object(
        &mut self,
        object: &Object,
        span: Span,
    ) -> Result<CompiledObject, Diagnostic> {
        let width = self.constant_number(&object.width, span, "object width")?;
        let depth = self.constant_number(&object.depth, span, "object depth")?;
        check_positive(width, span, "object width")?;
        check_positive(depth, span, "object depth")?;

        let mut scopes = Scopes::new();
        scopes.declare("w", Value::Number(width), false, span)?;
        scopes.declare("d", Value::Number(depth), false, span)?;
        let mut parts = Vec::new();
        self.object_block(&object.body, &mut scopes, &mut parts, width, depth)?;

        // Authoring origin: shift the local coordinates into the footprint, so
        // the rest of the pipeline always works with `0..w`, `0..d`.
        let shift = match object.origin {
            Origin::Back => [0.0, 0.0, 0.0],
            Origin::Center => [width / 2.0, depth / 2.0, 0.0],
            Origin::Front => [width, depth, 0.0],
        };
        if shift[0] != 0.0 || shift[1] != 0.0 {
            for part in &mut parts {
                *part = part.translated(shift);
            }
        }

        Ok(CompiledObject {
            name: object.name.clone(),
            width,
            depth,
            parts,
        })
    }

    fn object_block(
        &mut self,
        block: &Block,
        scopes: &mut Scopes,
        parts: &mut Vec<Part>,
        width: f64,
        depth: f64,
    ) -> Result<(), Diagnostic> {
        scopes.push();
        let mut result = Ok(());
        for statement in &block.statements {
            if let Err(error) = self.object_statement(statement, scopes, parts, width, depth) {
                result = Err(error);
                break;
            }
        }
        scopes.pop();
        result
    }

    // Loop counters are bounded by MAX_LOOP_BOUND (1e9), far below 2^53,
    // so the i64 -> f64 conversion is exact.
    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
    fn object_statement(
        &mut self,
        statement: &Statement,
        scopes: &mut Scopes,
        parts: &mut Vec<Part>,
        width: f64,
        depth: f64,
    ) -> Result<(), Diagnostic> {
        self.spend(statement.span)?;

        match &statement.kind {
            StatementKind::Let {
                name,
                value,
                mutable,
            } => {
                let value = self.expr(value, scopes)?;
                scopes.declare(name, value, *mutable, statement.span)?;
            }
            StatementKind::Assign { name, op, value } => {
                let value = self.assign_value(value, scopes)?;
                match op {
                    AssignOp::Set => scopes.assign(name, value, statement.span)?,
                    AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div => {
                        let current = scopes
                            .get(name)
                            .ok_or_else(|| {
                                diag(
                                    "E0301",
                                    format!("unknown variable `{name}`"),
                                    statement.span,
                                )
                            })?
                            .value;
                        let Value::Number(current) = current else {
                            return Err(diag(
                                "E0302",
                                "compound assignment requires a number",
                                statement.span,
                            ));
                        };
                        let Value::Number(value) = value else {
                            return Err(diag(
                                "E0302",
                                "compound assignment requires a number",
                                statement.span,
                            ));
                        };
                        let result = match op {
                            AssignOp::Add => current + value,
                            AssignOp::Sub => current - value,
                            AssignOp::Mul => current * value,
                            AssignOp::Div => {
                                if value == 0.0 {
                                    return Err(diag("E0308", "division by zero", statement.span));
                                }
                                current / value
                            }
                            AssignOp::Set => unreachable!(),
                        };
                        if !result.is_finite() {
                            return Err(diag("E0304", "number is out of range", statement.span));
                        }
                        scopes.assign(name, Value::Number(result), statement.span)?;
                    }
                }
            }
            StatementKind::For {
                variable,
                start,
                end,
                body,
            } => {
                let (start, end) = self.for_bounds(start, end, scopes, statement.span)?;
                for index in start..end {
                    self.spend(statement.span)?;
                    scopes.push();
                    scopes.declare(variable, Value::Number(index as f64), false, statement.span)?;
                    let mut result = Ok(());
                    for nested in &body.statements {
                        if let Err(error) =
                            self.object_statement(nested, scopes, parts, width, depth)
                        {
                            result = Err(error);
                            break;
                        }
                    }
                    scopes.pop();
                    result?;
                }
            }
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                let condition = self.boolean(condition, scopes)?;
                if condition {
                    self.object_block(then_block, scopes, parts, width, depth)?;
                } else {
                    match else_branch {
                        Some(ElseBranch::Block(block)) => {
                            self.object_block(block, scopes, parts, width, depth)?;
                        }
                        Some(ElseBranch::If(statement)) => {
                            self.object_statement(statement, scopes, parts, width, depth)?;
                        }
                        None => {}
                    }
                }
            }
            StatementKind::Shape(shape) => {
                let part = self.compile_shape(shape, scopes, statement.span)?;
                self.push_part(parts, part, statement.span)?;
            }
            StatementKind::Merge(block) => {
                let start = parts.len();
                self.object_block(block, scopes, parts, width, depth)?;
                self.merge_groups += 1;
                let group = self.merge_groups;
                for part in &mut parts[start..] {
                    if part.group.is_none() {
                        part.group = Some(group);
                    }
                }
            }
            StatementKind::Mirror(axis) => {
                self.mirror(parts, *axis, width, depth, statement.span)?;
            }
            StatementKind::Use {
                object,
                at,
                rotation,
            } => {
                let used = self.objects.get(object).cloned().ok_or_else(|| {
                    diag(
                        "E0301",
                        format!("unknown object `{object}`"),
                        statement.span,
                    )
                })?;
                let at = match at {
                    Some(values) => self.coordinates(values, scopes, statement.span)?,
                    None => [0.0, 0.0, 0.0],
                };
                let rotation = match rotation {
                    Some(expr) => {
                        normalize_rotation(self.number(expr, scopes, "rotation")?, expr.span)?
                    }
                    None => 0,
                };
                let copies: Vec<Part> = used
                    .parts
                    .iter()
                    .map(|part| {
                        part.rotated(rotation, used.width, used.depth)
                            .translated(at)
                    })
                    .collect();
                if self.shapes.saturating_add(copies.len()) > MAX_SHAPES {
                    return Err(diag(
                        "E0402",
                        format!("more than {MAX_SHAPES} shapes"),
                        statement.span,
                    ));
                }
                self.shapes += copies.len();
                parts.extend(copies);
            }
            StatementKind::Place { .. } => {
                return Err(diag(
                    "E0302",
                    "`place` is not allowed inside an object",
                    statement.span,
                ));
            }
        }
        Ok(())
    }

    fn scene_block(
        &mut self,
        block: &Block,
        scopes: &mut Scopes,
        instances: &mut Vec<Instance>,
    ) -> Result<(), Diagnostic> {
        scopes.push();
        let mut result = Ok(());
        for statement in &block.statements {
            if let Err(error) = self.scene_statement(statement, scopes, instances) {
                result = Err(error);
                break;
            }
        }
        scopes.pop();
        result
    }

    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
    fn scene_statement(
        &mut self,
        statement: &Statement,
        scopes: &mut Scopes,
        instances: &mut Vec<Instance>,
    ) -> Result<(), Diagnostic> {
        self.spend(statement.span)?;

        match &statement.kind {
            StatementKind::Let {
                name,
                value,
                mutable,
            } => {
                let value = self.expr(value, scopes)?;
                scopes.declare(name, value, *mutable, statement.span)?;
            }
            StatementKind::Assign { name, op, value } => {
                let value = self.assign_value(value, scopes)?;
                match op {
                    AssignOp::Set => scopes.assign(name, value, statement.span)?,
                    AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div => {
                        let current = scopes
                            .get(name)
                            .ok_or_else(|| {
                                diag(
                                    "E0301",
                                    format!("unknown variable `{name}`"),
                                    statement.span,
                                )
                            })?
                            .value;
                        let Value::Number(current) = current else {
                            return Err(diag(
                                "E0302",
                                "compound assignment requires a number",
                                statement.span,
                            ));
                        };
                        let Value::Number(value) = value else {
                            return Err(diag(
                                "E0302",
                                "compound assignment requires a number",
                                statement.span,
                            ));
                        };
                        let result = match op {
                            AssignOp::Add => current + value,
                            AssignOp::Sub => current - value,
                            AssignOp::Mul => current * value,
                            AssignOp::Div => {
                                if value == 0.0 {
                                    return Err(diag("E0308", "division by zero", statement.span));
                                }
                                current / value
                            }
                            AssignOp::Set => unreachable!(),
                        };
                        if !result.is_finite() {
                            return Err(diag("E0304", "number is out of range", statement.span));
                        }
                        scopes.assign(name, Value::Number(result), statement.span)?;
                    }
                }
            }
            StatementKind::For {
                variable,
                start,
                end,
                body,
            } => {
                let (start, end) = self.for_bounds(start, end, scopes, statement.span)?;
                for index in start..end {
                    self.spend(statement.span)?;
                    scopes.push();
                    scopes.declare(variable, Value::Number(index as f64), false, statement.span)?;
                    let mut result = Ok(());
                    for nested in &body.statements {
                        if let Err(error) = self.scene_statement(nested, scopes, instances) {
                            result = Err(error);
                            break;
                        }
                    }
                    scopes.pop();
                    result?;
                }
            }
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                let condition = self.boolean(condition, scopes)?;
                if condition {
                    self.scene_block(then_block, scopes, instances)?;
                } else {
                    match else_branch {
                        Some(ElseBranch::Block(block)) => {
                            self.scene_block(block, scopes, instances)?;
                        }
                        Some(ElseBranch::If(statement)) => {
                            self.scene_statement(statement, scopes, instances)?;
                        }
                        None => {}
                    }
                }
            }
            StatementKind::Place {
                object,
                at,
                rotation,
                turn,
                anchor,
            } => {
                let object = self.objects.get(object).cloned().ok_or_else(|| {
                    diag(
                        "E0301",
                        format!("unknown object `{object}`"),
                        statement.span,
                    )
                })?;
                let at = self.coordinates(at, scopes, statement.span)?;
                let rotation = match rotation {
                    Some(expr) => {
                        normalize_rotation(self.number(expr, scopes, "rotation")?, expr.span)?
                    }
                    None => 0,
                };
                let angle = match turn {
                    Some(expr) => self.number(expr, scopes, "turn")?,
                    None => 0.0,
                };
                if self.instances >= MAX_INSTANCES {
                    return Err(diag(
                        "E0402",
                        format!("more than {MAX_INSTANCES} placements"),
                        statement.span,
                    ));
                }
                self.instances += 1;
                let mut instance = Instance {
                    object,
                    at,
                    rotation,
                    angle,
                };
                if let Some(anchor) = anchor {
                    let zero = Instance {
                        object: instance.object.clone(),
                        at: [0.0, 0.0, 0.0],
                        rotation,
                        angle: 0.0,
                    };
                    let bounds = zero.world_bounds();
                    let corner = match anchor {
                        Anchor::Back => [bounds.min[0], bounds.min[1]],
                        Anchor::Center => [
                            bounds.min[0].midpoint(bounds.max[0]),
                            bounds.min[1].midpoint(bounds.max[1]),
                        ],
                        Anchor::Front => [bounds.max[0], bounds.max[1]],
                        Anchor::Left => [bounds.min[0], bounds.max[1]],
                        Anchor::Right => [bounds.max[0], bounds.min[1]],
                        Anchor::Cell => [bounds.max[0] - 1.0, bounds.max[1] - 1.0],
                    };
                    instance.at = [at[0] - corner[0], at[1] - corner[1], at[2]];
                }
                instances.push(instance);
            }
            StatementKind::Shape(_)
            | StatementKind::Mirror(_)
            | StatementKind::Use { .. }
            | StatementKind::Merge(_) => {
                return Err(diag(
                    "E0302",
                    "shapes, mirror, use and merge are only allowed inside an object",
                    statement.span,
                ));
            }
        }
        Ok(())
    }

    fn compile_shape(
        &self,
        shape: &Shape,
        scopes: &Scopes,
        span: Span,
    ) -> Result<Part, Diagnostic> {
        let at = self.coordinates(shape.at(), scopes, span)?;
        let material = shape.material().map(str::to_owned);
        if let Some(name) = &material {
            if !self.material_known(name) {
                return Err(diag("E0305", format!("unknown material `{name}`"), span));
            }
        }

        let primitive = match shape {
            Shape::Box { size, .. } => Primitive::Box {
                at,
                size: self.positive_triple(size, scopes, span, "box size")?,
            },
            Shape::BoxBetween { from, to, .. } => {
                let start = self.coordinates(from, scopes, span)?;
                let end = self.coordinates(to, scopes, span)?;
                let size = [
                    (end[0] - start[0]).abs(),
                    (end[1] - start[1]).abs(),
                    (end[2] - start[2]).abs(),
                ];
                if size.iter().any(|value| *value <= 0.0) {
                    return Err(diag(
                        "E0304",
                        "box from/to must span a positive size on every axis",
                        span,
                    ));
                }
                Primitive::Box {
                    at: [
                        start[0].min(end[0]),
                        start[1].min(end[1]),
                        start[2].min(end[2]),
                    ],
                    size,
                }
            }
            Shape::Cyl { radius, height, .. } => Primitive::Cyl {
                at,
                radius: self.positive(radius, scopes, span, "radius")?,
                height: self.positive(height, scopes, span, "height")?,
            },
            Shape::Sphere { radius, .. } => Primitive::Sphere {
                at,
                radius: self.positive(radius, scopes, span, "radius")?,
            },
            Shape::SlantBox { size, offset, .. } => Primitive::SlantBox {
                at,
                size: self.positive_triple(size, scopes, span, "slantbox size")?,
                offset: self.offsets(offset, scopes, span)?,
            },
            Shape::SlantCyl {
                radius,
                height,
                offset,
                ..
            } => Primitive::SlantCyl {
                at,
                radius: self.positive(radius, scopes, span, "radius")?,
                height: self.positive(height, scopes, span, "height")?,
                offset: self.offsets(offset, scopes, span)?,
            },
            Shape::Ramp { size, rise, .. } => Primitive::Ramp {
                at,
                size: self.positive_pair(size, scopes, span, "ramp size")?,
                rise: self.offsets(rise, scopes, span)?,
            },
        };
        Ok(Part {
            primitive,
            material,
            group: None,
        })
    }

    fn push_part(
        &mut self,
        parts: &mut Vec<Part>,
        part: Part,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self.shapes >= MAX_SHAPES {
            return Err(diag(
                "E0402",
                format!("more than {MAX_SHAPES} shapes"),
                span,
            ));
        }
        self.shapes += 1;
        parts.push(part);
        Ok(())
    }

    fn mirror(
        &mut self,
        parts: &mut Vec<Part>,
        axis: Axis,
        width: f64,
        depth: f64,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let copies: Vec<Part> = parts
            .iter()
            .map(|part| part.mirrored(axis, width, depth))
            .collect();
        if self.shapes.saturating_add(copies.len()) > MAX_SHAPES {
            return Err(diag(
                "E0402",
                format!("more than {MAX_SHAPES} shapes"),
                span,
            ));
        }
        self.shapes += copies.len();
        parts.extend(copies);
        Ok(())
    }

    fn coordinates(
        &self,
        values: &[Expr; 3],
        scopes: &Scopes,
        span: Span,
    ) -> Result<[f64; 3], Diagnostic> {
        let x = self.number(&values[0], scopes, "coordinate")?;
        let y = self.number(&values[1], scopes, "coordinate")?;
        let z = self.number(&values[2], scopes, "coordinate")?;
        for value in [x, y, z] {
            check_coord(value, span)?;
        }
        Ok([x, y, z])
    }

    fn positive_triple(
        &self,
        values: &[Expr; 3],
        scopes: &Scopes,
        span: Span,
        what: &str,
    ) -> Result<[f64; 3], Diagnostic> {
        let x = self.positive(&values[0], scopes, span, what)?;
        let y = self.positive(&values[1], scopes, span, what)?;
        let z = self.positive(&values[2], scopes, span, what)?;
        Ok([x, y, z])
    }

    fn positive_pair(
        &self,
        values: &[Expr; 2],
        scopes: &Scopes,
        span: Span,
        what: &str,
    ) -> Result<[f64; 2], Diagnostic> {
        let x = self.positive(&values[0], scopes, span, what)?;
        let y = self.positive(&values[1], scopes, span, what)?;
        Ok([x, y])
    }

    fn offsets(
        &self,
        values: &[Expr; 2],
        scopes: &Scopes,
        span: Span,
    ) -> Result<[f64; 2], Diagnostic> {
        let x = self.number(&values[0], scopes, "offset")?;
        let y = self.number(&values[1], scopes, "offset")?;
        for value in [x, y] {
            if !value.is_finite() || value.abs() > MAX_COORD {
                return Err(diag("E0304", "offset is out of range", span));
            }
        }
        Ok([x, y])
    }

    fn positive(
        &self,
        expr: &Expr,
        scopes: &Scopes,
        span: Span,
        what: &str,
    ) -> Result<f64, Diagnostic> {
        let value = self.number(expr, scopes, what)?;
        check_positive(value, span, what)?;
        Ok(value)
    }

    fn constant_number(&self, expr: &Expr, span: Span, what: &str) -> Result<f64, Diagnostic> {
        if !is_constant(expr) {
            return Err(diag(
                "E0310",
                format!("{what} must be a constant expression"),
                span,
            ));
        }
        let scopes = Scopes::new();
        self.number(expr, &scopes, what)
    }

    fn constant_tuple3(
        &self,
        values: &[Expr; 3],
        span: Span,
        what: &str,
    ) -> Result<[f64; 3], Diagnostic> {
        let x = self.constant_number(&values[0], span, what)?;
        let y = self.constant_number(&values[1], span, what)?;
        let z = self.constant_number(&values[2], span, what)?;
        Ok([x, y, z])
    }

    fn expr(&self, expr: &Expr, scopes: &Scopes) -> Result<Value, Diagnostic> {
        match &expr.kind {
            ExprKind::Number(value) => Ok(Value::Number(*value)),
            ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            ExprKind::Ident(name) => scopes
                .get(name)
                .map(|variable| variable.value)
                .ok_or_else(|| diag("E0301", format!("unknown variable `{name}`"), expr.span)),
            ExprKind::Unary { op, operand } => {
                let value = self.expr(operand, scopes)?;
                match (op, value) {
                    (UnaryOp::Negate, Value::Number(value)) => {
                        let result = -value;
                        if !result.is_finite() {
                            return Err(diag("E0304", "number is out of range", expr.span));
                        }
                        Ok(Value::Number(result))
                    }
                    (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
                    _ => Err(diag(
                        "E0302",
                        format!("unary operator cannot be applied to {}", value.type_name()),
                        expr.span,
                    )),
                }
            }
            ExprKind::Binary { op, left, right } => {
                self.binary(*op, left, right, scopes, expr.span)
            }
        }
    }

    #[allow(clippy::float_cmp)]
    fn binary(
        &self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        scopes: &Scopes,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if op == BinaryOp::And {
            let left = self.boolean(left, scopes)?;
            return if left {
                self.boolean(right, scopes).map(Value::Bool)
            } else {
                Ok(Value::Bool(false))
            };
        }
        if op == BinaryOp::Or {
            let left = self.boolean(left, scopes)?;
            return if left {
                Ok(Value::Bool(true))
            } else {
                self.boolean(right, scopes).map(Value::Bool)
            };
        }

        let left = self.number(left, scopes, "operand")?;
        let right = self.number(right, scopes, "operand")?;
        match op {
            BinaryOp::Eq => Ok(Value::Bool(left == right)),
            BinaryOp::Ne => Ok(Value::Bool(left != right)),
            BinaryOp::Lt => Ok(Value::Bool(left < right)),
            BinaryOp::Le => Ok(Value::Bool(left <= right)),
            BinaryOp::Gt => Ok(Value::Bool(left > right)),
            BinaryOp::Ge => Ok(Value::Bool(left >= right)),
            BinaryOp::Add => finite(left + right, span),
            BinaryOp::Sub => finite(left - right, span),
            BinaryOp::Mul => finite(left * right, span),
            BinaryOp::Div => {
                if right == 0.0 {
                    Err(diag("E0308", "division by zero", span))
                } else {
                    finite(left / right, span)
                }
            }
            BinaryOp::Rem => {
                if right == 0.0 {
                    Err(diag("E0308", "division by zero", span))
                } else {
                    finite(left % right, span)
                }
            }
            BinaryOp::And | BinaryOp::Or => Err(diag("E0302", "unexpected logical operator", span)),
        }
    }

    fn number(&self, expr: &Expr, scopes: &Scopes, what: &str) -> Result<f64, Diagnostic> {
        match self.expr(expr, scopes)? {
            Value::Number(value) => Ok(value),
            Value::Bool(_) => Err(diag(
                "E0302",
                format!("{what} must be a number, found bool"),
                expr.span,
            )),
        }
    }

    fn boolean(&self, expr: &Expr, scopes: &Scopes) -> Result<bool, Diagnostic> {
        match self.expr(expr, scopes)? {
            Value::Bool(value) => Ok(value),
            Value::Number(_) => Err(diag(
                "E0302",
                "condition must be a bool, found number",
                expr.span,
            )),
        }
    }

    fn assign_value(&self, expr: &Expr, scopes: &Scopes) -> Result<Value, Diagnostic> {
        self.expr(expr, scopes)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn for_bounds(
        &self,
        start: &Expr,
        end: &Expr,
        scopes: &Scopes,
        span: Span,
    ) -> Result<(i64, i64), Diagnostic> {
        let start = self.number(start, scopes, "loop bound")?;
        let end = self.number(end, scopes, "loop bound")?;
        for value in [start, end] {
            if !value.is_finite() || value.fract() != 0.0 || value.abs() > MAX_LOOP_BOUND {
                return Err(diag(
                    "E0309",
                    "loop bounds must be integers with absolute value at most 1e9",
                    span,
                ));
            }
        }
        Ok((start as i64, end as i64))
    }

    fn material_known(&self, name: &str) -> bool {
        is_builtin(name) || self.materials.contains_key(name)
    }

    fn load_include(
        &mut self,
        include: &Include,
        base_dir: &Path,
        root: &Path,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self.include_stack.len() >= MAX_INCLUDE_DEPTH {
            return Err(diag(
                "E0403",
                format!("include depth exceeds {MAX_INCLUDE_DEPTH}"),
                span,
            ));
        }
        let path = resolve_include(&include.path, base_dir, root, span)?;
        if self.include_stack.contains(&path) {
            return Err(diag(
                "E0503",
                format!("include cycle at `{}`", path.display()),
                span,
            ));
        }
        if path.extension().and_then(|value| value.to_str()) != Some("model") {
            return Err(diag(
                "E0503",
                "included files must have the .model extension",
                span,
            ));
        }
        let metadata = fs::metadata(&path).map_err(|error| {
            diag(
                "E0501",
                format!("cannot read `{}`: {error}", path.display()),
                span,
            )
        })?;
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        self.included_bytes = self.included_bytes.saturating_add(length);
        if self.included_bytes > MAX_INCLUDED_BYTES {
            return Err(diag(
                "E0402",
                format!("included files exceed {MAX_INCLUDED_BYTES} bytes"),
                span,
            ));
        }
        let source = fs::read_to_string(&path).map_err(|error| {
            diag(
                "E0501",
                format!("cannot read `{}`: {error}", path.display()),
                span,
            )
        })?;
        let program = parse(&source)?;

        let model_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        self.include_stack.push(path);
        let result = self.included_model(&program, span, &model_dir);
        self.include_stack.pop();
        result
    }

    fn included_model(
        &mut self,
        program: &Program,
        span: Span,
        base_dir: &Path,
    ) -> Result<(), Diagnostic> {
        for item in &program.items {
            if let ItemKind::Material(material) = &item.kind {
                self.add_material(material, item.span, base_dir)?;
            }
        }
        for item in &program.items {
            match &item.kind {
                ItemKind::Material(_) => {}
                ItemKind::Object(object) => self.add_object(object, item.span)?,
                _ => {
                    return Err(diag(
                        "E0503",
                        "an included .model file may contain only material and object",
                        span,
                    ));
                }
            }
        }
        Ok(())
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn normalize_rotation(value: f64, span: Span) -> Result<u16, Diagnostic> {
    let normalized = value.rem_euclid(360.0);
    if normalized.fract() != 0.0 {
        return Err(diag("E0306", "rotation must be 0, 90, 180 or 270", span));
    }
    match normalized as u16 {
        0 => Ok(0),
        90 => Ok(90),
        180 => Ok(180),
        270 => Ok(270),
        _ => Err(diag("E0306", "rotation must be 0, 90, 180 or 270", span)),
    }
}

fn diag(code: &'static str, message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new(code, message, span)
}

fn finite(value: f64, span: Span) -> Result<Value, Diagnostic> {
    if value.is_finite() {
        Ok(Value::Number(value))
    } else {
        Err(diag("E0304", "number is out of range", span))
    }
}

fn check_coord(value: f64, span: Span) -> Result<(), Diagnostic> {
    if value.is_finite() && value.abs() <= MAX_COORD {
        Ok(())
    } else {
        Err(diag("E0304", "coordinate is out of range", span))
    }
}

fn check_positive(value: f64, span: Span, what: &str) -> Result<(), Diagnostic> {
    if value.is_finite() && value > 0.0 && value <= MAX_COORD {
        Ok(())
    } else {
        Err(diag(
            "E0307",
            format!("{what} must be positive and finite"),
            span,
        ))
    }
}

fn is_constant(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Number(_) | ExprKind::Bool(_) => true,
        ExprKind::Ident(_) => false,
        ExprKind::Unary { operand, .. } => is_constant(operand),
        ExprKind::Binary { left, right, .. } => is_constant(left) && is_constant(right),
    }
}

fn is_builtin(name: &str) -> bool {
    BUILTIN_MATERIALS.contains(&name)
}

fn set_once<T>(slot: &mut Option<T>, value: T, span: Span, what: &str) -> Result<(), Diagnostic> {
    if slot.is_some() {
        return Err(diag(
            "E0303",
            format!("`{what}` is declared more than once"),
            span,
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn safe_relative(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn resolve_include(
    path: &str,
    base_dir: &Path,
    root: &Path,
    span: Span,
) -> Result<PathBuf, Diagnostic> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(diag(
            "E0502",
            format!("include path `{path}` must be relative"),
            span,
        ));
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let resolved = base_dir.join(relative).canonicalize().map_err(|error| {
        diag(
            "E0501",
            format!("cannot resolve include `{path}`: {error}"),
            span,
        )
    })?;
    if !resolved.starts_with(&root) {
        return Err(diag(
            "E0502",
            format!("include path `{path}` escapes the project root"),
            span,
        ));
    }
    Ok(resolved)
}
