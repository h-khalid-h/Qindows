//! # Q-Shell Scripting Language
//!
//! A lightweight scripting language for Q-Shell.
//! Supports variables, control flow, functions, and pipes.
//!
//! Example:
//! ```qsh
//! let files = ls("~/Documents")
//! for f in files {
//!     if f.ends_with(".txt") {
//!         echo(f)
//!     }
//! }
//! ```

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Script value types.
#[derive(Debug, Clone)]
pub enum Value {
    /// Nothing
    Nil,
    /// Boolean
    Bool(bool),
    /// Integer
    Int(i64),
    /// Float
    Float(f64),
    /// String
    Str(String),
    /// List of values
    List(Vec<Value>),
    /// Map of key-value pairs
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Value::Nil => String::from("nil"),
            Value::Bool(b) => alloc::format!("{}", b),
            Value::Int(n) => alloc::format!("{}", n),
            Value::Float(f) => alloc::format!("{:.6}", f),
            Value::Str(s) => s.clone(),
            Value::List(l) => alloc::format!("[{} items]", l.len()),
            Value::Map(m) => alloc::format!("{{{} entries}}", m.len()),
        }
    }
}

/// AST node types.
#[derive(Debug, Clone)]
pub enum AstNode {
    /// Literal value
    Literal(Value),
    /// Variable reference
    Var(String),
    /// Let binding: let name = expr
    Let { name: String, value: Box<AstNode> },
    /// Assignment: name = expr
    Assign { name: String, value: Box<AstNode> },
    /// Binary operation
    BinOp { left: Box<AstNode>, op: BinOp, right: Box<AstNode> },
    /// Unary operation
    UnaryOp { op: UnaryOp, operand: Box<AstNode> },
    /// Function call
    Call { name: String, args: Vec<AstNode> },
    /// If expression
    If { condition: Box<AstNode>, then: Vec<AstNode>, else_: Option<Vec<AstNode>> },
    /// For loop
    For { var: String, iter: Box<AstNode>, body: Vec<AstNode> },
    /// While loop
    While { condition: Box<AstNode>, body: Vec<AstNode> },
    /// Function definition
    FnDef { name: String, params: Vec<String>, body: Vec<AstNode> },
    /// Return
    Return(Box<AstNode>),
    /// Pipe: expr | expr
    Pipe { left: Box<AstNode>, right: Box<AstNode> },
    /// Block of statements
    Block(Vec<AstNode>),
    /// Index access: expr[expr]
    Index { object: Box<AstNode>, index: Box<AstNode> },
    /// Field access: expr.field
    Field { object: Box<AstNode>, field: String },
}

/// Binary operators.
#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Gt, Le, Ge,
    And, Or,
    Concat, // String concatenation ++
}

/// Unary operators.
#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// A variable scope.
#[derive(Debug, Clone)]
pub struct Scope {
    /// Variables
    pub vars: BTreeMap<String, Value>,
    /// Parent scope index
    pub parent: Option<usize>,
}

/// A user-defined function.
#[derive(Debug, Clone)]
pub struct UserFunction {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<AstNode>,
}

/// Script execution errors.
#[derive(Debug, Clone)]
pub enum ScriptError {
    UndefinedVariable(String),
    UndefinedFunction(String),
    TypeError(String),
    DivisionByZero,
    IndexOutOfBounds,
    MaxDepthExceeded,
    BreakSignal,
    ReturnSignal(Value),
}

/// The Q-Shell Script Interpreter.
pub struct Interpreter {
    /// Scope stack
    pub scopes: Vec<Scope>,
    /// User-defined functions
    pub functions: BTreeMap<String, UserFunction>,
    /// Output buffer
    pub output: Vec<String>,
    /// Max recursion depth
    pub max_depth: usize,
    /// Current depth
    pub depth: usize,
}

impl Interpreter {
    pub fn new() -> Self {
        let global = Scope {
            vars: BTreeMap::new(),
            parent: None,
        };

        Interpreter {
            scopes: alloc::vec![global],
            functions: BTreeMap::new(),
            output: Vec::new(),
            max_depth: 64,
            depth: 0,
        }
    }

    /// Execute a list of AST nodes.
    pub fn exec(&mut self, nodes: &[AstNode]) -> Result<Value, ScriptError> {
        let mut result = Value::Nil;
        for node in nodes {
            result = self.eval(node)?;
        }
        Ok(result)
    }

    /// Evaluate a single AST node.
    pub fn eval(&mut self, node: &AstNode) -> Result<Value, ScriptError> {
        match node {
            AstNode::Literal(v) => Ok(v.clone()),

            AstNode::Var(name) => self.lookup(name).cloned()
                .ok_or_else(|| ScriptError::UndefinedVariable(name.clone())),

            AstNode::Let { name, value } => {
                let val = self.eval(value)?;
                self.current_scope_mut().vars.insert(name.clone(), val.clone());
                Ok(val)
            }

            AstNode::Assign { name, value } => {
                let val = self.eval(value)?;
                // Search up the scope chain
                for i in (0..self.scopes.len()).rev() {
                    if self.scopes[i].vars.contains_key(name) {
                        self.scopes[i].vars.insert(name.clone(), val.clone());
                        return Ok(val);
                    }
                }
                Err(ScriptError::UndefinedVariable(name.clone()))
            }

            AstNode::BinOp { left, op, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                self.eval_binop(&l, *op, &r)
            }

            AstNode::UnaryOp { op, operand } => {
                let v = self.eval(operand)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(ScriptError::TypeError(String::from("Cannot negate"))),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.is_truthy())),
                }
            }

            AstNode::Call { name, args } => {
                let evaluated_args: Vec<Value> = args.iter()
                    .map(|a| self.eval(a))
                    .collect::<Result<_, _>>()?;
                self.call_function(name, &evaluated_args)
            }

            AstNode::If { condition, then, else_ } => {
                let cond = self.eval(condition)?;
                if cond.is_truthy() {
                    self.exec(then)
                } else if let Some(else_body) = else_ {
                    self.exec(else_body)
                } else {
                    Ok(Value::Nil)
                }
            }

            AstNode::For { var, iter, body } => {
                let iterable = self.eval(iter)?;
                if let Value::List(items) = iterable {
                    let mut result = Value::Nil;
                    for item in items {
                        self.current_scope_mut().vars.insert(var.clone(), item);
                        result = self.exec(body)?;
                    }
                    Ok(result)
                } else {
                    Err(ScriptError::TypeError(String::from("Not iterable")))
                }
            }

            AstNode::While { condition, body } => {
                let mut result = Value::Nil;
                loop {
                    let cond = self.eval(condition)?;
                    if !cond.is_truthy() { break; }
                    result = self.exec(body)?;
                }
                Ok(result)
            }

            AstNode::FnDef { name, params, body } => {
                self.functions.insert(name.clone(), UserFunction {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                });
                Ok(Value::Nil)
            }

            AstNode::Return(expr) => {
                let val = self.eval(expr)?;
                Err(ScriptError::ReturnSignal(val))
            }

            AstNode::Block(stmts) => self.exec(stmts),

            AstNode::Pipe { left, right } => {
                let left_val = self.eval(left)?;
                // Set $_ to the pipe input
                self.current_scope_mut().vars.insert(String::from("_"), left_val);
                self.eval(right)
            }

            _ => Ok(Value::Nil),
        }
    }

    /// Evaluate a binary operation.
    fn eval_binop(&self, left: &Value, op: BinOp, right: &Value) -> Result<Value, ScriptError> {
        match (left, op, right) {
            (Value::Int(a), BinOp::Add, Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Int(a), BinOp::Sub, Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Int(a), BinOp::Mul, Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Int(a), BinOp::Div, Value::Int(b)) => {
                if *b == 0 { Err(ScriptError::DivisionByZero) }
                else { Ok(Value::Int(a / b)) }
            }
            (Value::Int(a), BinOp::Mod, Value::Int(b)) => Ok(Value::Int(a % b)),
            (Value::Int(a), BinOp::Eq, Value::Int(b)) => Ok(Value::Bool(a == b)),
            (Value::Int(a), BinOp::Ne, Value::Int(b)) => Ok(Value::Bool(a != b)),
            (Value::Int(a), BinOp::Lt, Value::Int(b)) => Ok(Value::Bool(a < b)),
            (Value::Int(a), BinOp::Gt, Value::Int(b)) => Ok(Value::Bool(a > b)),
            (Value::Int(a), BinOp::Le, Value::Int(b)) => Ok(Value::Bool(a <= b)),
            (Value::Int(a), BinOp::Ge, Value::Int(b)) => Ok(Value::Bool(a >= b)),

            (Value::Str(a), BinOp::Concat, Value::Str(b)) => {
                let mut s = a.clone();
                s.push_str(b);
                Ok(Value::Str(s))
            }
            (Value::Str(a), BinOp::Eq, Value::Str(b)) => Ok(Value::Bool(a == b)),

            (_, BinOp::And, _) => Ok(Value::Bool(left.is_truthy() && right.is_truthy())),
            (_, BinOp::Or, _) => Ok(Value::Bool(left.is_truthy() || right.is_truthy())),

            _ => Err(ScriptError::TypeError(String::from("Type mismatch in binary op"))),
        }
    }

    /// Call a function (built-in or user-defined).
    fn call_function(&mut self, name: &str, args: &[Value]) -> Result<Value, ScriptError> {
        // Built-in functions
        match name {
            "echo" => {
                let text = args.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(" ");
                self.output.push(text);
                Ok(Value::Nil)
            }
            "len" => {
                match args.first() {
                    Some(Value::Str(s)) => Ok(Value::Int(s.len() as i64)),
                    Some(Value::List(l)) => Ok(Value::Int(l.len() as i64)),
                    _ => Ok(Value::Int(0)),
                }
            }
            "type" => {
                let t = match args.first() {
                    Some(Value::Nil) => "nil",
                    Some(Value::Bool(_)) => "bool",
                    Some(Value::Int(_)) => "int",
                    Some(Value::Float(_)) => "float",
                    Some(Value::Str(_)) => "string",
                    Some(Value::List(_)) => "list",
                    Some(Value::Map(_)) => "map",
                    None => "nil",
                };
                Ok(Value::Str(String::from(t)))
            }
            "range" => {
                let start = match args.first() { Some(Value::Int(n)) => *n, _ => 0 };
                let end = match args.get(1) { Some(Value::Int(n)) => *n, _ => 0 };
                let list: Vec<Value> = (start..end).map(Value::Int).collect();
                Ok(Value::List(list))
            }
            "push" => {
                if let (Some(Value::List(mut list)), Some(item)) = (args.first().cloned(), args.get(1)) {
                    list.push(item.clone());
                    Ok(Value::List(list))
                } else {
                    Err(ScriptError::TypeError(String::from("push requires list")))
                }
            }
            _ => {
                // User-defined function
                if let Some(func) = self.functions.get(name).cloned() {
                    self.depth += 1;
                    if self.depth > self.max_depth {
                        self.depth -= 1;
                        return Err(ScriptError::MaxDepthExceeded);
                    }

                    // Create new scope
                    let mut scope = Scope { vars: BTreeMap::new(), parent: Some(self.scopes.len() - 1) };
                    for (i, param) in func.params.iter().enumerate() {
                        scope.vars.insert(param.clone(), args.get(i).cloned().unwrap_or(Value::Nil));
                    }
                    self.scopes.push(scope);

                    let result = match self.exec(&func.body) {
                        Ok(v) => Ok(v),
                        Err(ScriptError::ReturnSignal(v)) => Ok(v),
                        Err(e) => Err(e),
                    };

                    self.scopes.pop();
                    self.depth -= 1;
                    result
                } else {
                    Err(ScriptError::UndefinedFunction(String::from(name)))
                }
            }
        }
    }

    /// Look up a variable in the scope chain.
    fn lookup(&self, name: &str) -> Option<&Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.vars.get(name) {
                return Some(val);
            }
        }
        None
    }

    /// Get the current (topmost) scope.
    fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }

    /// Get all output lines.
    pub fn drain_output(&mut self) -> Vec<String> {
        core::mem::take(&mut self.output)
    }
}
