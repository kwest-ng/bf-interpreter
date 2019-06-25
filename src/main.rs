use byteorder::WriteBytesExt;

use std::fs::File;
use std::io::{prelude::*, stdin, stdout};
use std::path::Path;
use std::thread;
use std::time::Duration;

const ARRAY_SIZE: usize = u16::max_value() as usize;

#[derive(Debug)]
enum OpCode {
    MoveForward,
    MoveBack,
    Increment,
    Decrement,
    Output,
    Input,
    JmpStart,
    JmpEnd,
}

#[derive(Debug)]
enum ModifyDirection {
    Up,
    Down,
}

#[derive(Debug)]
enum JumpFrom {
    Start,
    End,
}

#[derive(Debug)]
enum Action {
    JumpForward,
    JumpBack,
    Exit(String),
    None
}

#[derive(Debug)]
struct BfArray {
    raw: Vec<u8>,
    pointer: usize,
}

impl Default for BfArray {
    fn default() -> Self {
        Self::new()
    }
}

impl BfArray {
    pub fn new() -> Self {
        let mut raw = Vec::with_capacity(ARRAY_SIZE);
        raw.resize(ARRAY_SIZE, 0);
        Self {
            raw,
            pointer: Default::default()
        }
    }

    pub fn write_array(&self, to_cell: usize) {
        let mut file = File::create("visualizer").unwrap();
        
        for i in 0..=to_cell {
            write!(file, "{:>4}", i).unwrap();
        }
        writeln!(file).unwrap();
        
        for i in 0..=to_cell {
            write!(file, "{:>4}", self.raw[i]).unwrap();
        }
        writeln!(file).unwrap();

        writeln!(file, "{0:>1$}", "^", (self.pointer * 4) + 4).unwrap();
    }

    pub fn perform_operation<W: Write>(&mut self, opcode: &OpCode, writer: &mut W) -> Action {
        use OpCode::*;
        use JumpFrom::*;
        use ModifyDirection::*;

        let action = match opcode {
            Output => self.output(writer),
            Input => self.input(),
            JmpStart => self.jump_from(Start),
            JmpEnd => self.jump_from(End),
            Increment => self.modify_value(Up),
            Decrement => self.modify_value(Down),
            MoveForward => self.move_pointer(Up),
            MoveBack => self.move_pointer(Down),
        };

        action
    }

    #[inline]
    fn value(&self) -> u8 {
        self.raw[self.pointer]
    }

    #[inline]
    fn set_value(&mut self, val: u8) {
        self.raw[self.pointer] = val;
    }

    fn output<W: Write>(&self, writer: &mut W) -> Action {
        writer.write_u8(self.value()).expect("Write error");
        writer.flush().unwrap();
        Action::None
    }

    fn input(&mut self) -> Action {
        let mut input = 0;
        for byte in stdin().lock().bytes() {
            if byte.is_ok() {
                input = byte.unwrap();
                break;
            }
        };

        self.set_value(input);
        Action::None
    }

    fn jump_from(&self, from: JumpFrom) -> Action {
        let nonzero = self.value() != 0;
        match (from, nonzero) {
            (JumpFrom::Start, false) => Action::JumpForward,
            (JumpFrom::End, true) => Action::JumpBack,
            _ => Action::None
        }
    }

    fn modify_value(&mut self, direction: ModifyDirection) -> Action {
        let mod_func = match direction {
            ModifyDirection::Up => {
                u8::wrapping_add
            },
            ModifyDirection::Down => {
                u8::wrapping_sub
            },
        };

        self.set_value(mod_func(self.value(), 1));
        Action::None
    }

    fn move_pointer(&mut self, direction: ModifyDirection) -> Action {
        let mod_func = match direction {
            ModifyDirection::Up => {
                usize::checked_add
            },
            ModifyDirection::Down => {
                usize::checked_sub
            },
        };

        match mod_func(self.pointer, 1) {
            None => Action::Exit("Pointer access violation".into()),
            Some(x) => {
                self.pointer = x;
                Action::None
            }
        }
    }
}

#[derive(Debug)]
struct Interpreter {
    inner: BfArray,
    ops: Vec<OpCode>,
    jump_stack: Vec<usize>,
    pointer: usize,
}

impl Interpreter {
    pub fn new(ops: Vec<OpCode>) -> Self {
        Self {
            inner: Default::default(),
            ops,
            jump_stack: Default::default(),
            pointer: Default::default(),
        }
    }

    pub fn execute_all<W: Write>(&mut self, writer: &mut W) {
        self.pointer = 0;
        let wait = std::env::var("BF_VISUALIZER_TIME").map(|s| s.parse().unwrap_or(0)).unwrap_or(0);

        while let Some(op) = self.ops.get(self.pointer) {
            match self.inner.perform_operation(op, writer) {
                Action::None => {}
                Action::Exit(s) => {
                    eprintln!("{}", s);
                    break;
                },
                Action::JumpForward => {
                    self.jmp_forward();
                    continue;
                },
                Action::JumpBack => {
                    self.pointer = *self.jump_stack.last().unwrap();
                    continue;
                }
            }

            // If Action::None
            match op {
                OpCode::JmpStart => {
                    // Jump back should land on Op after current
                    self.jump_stack.push(self.pointer + 1);
                }
                OpCode::JmpEnd => {
                    self.jump_stack.pop().unwrap();
                }
                _ => {
                    if wait > 0 {
                        self.inner.write_array(6);
                        thread::sleep(Duration::from_millis(wait));
                    }
                }
            }

            self.increment_pointer();
        }
    }

    #[inline]
    fn increment_pointer(&mut self) {
        match self.pointer.checked_add(1) {
            Some(x) => {self.pointer = x;}
            None => panic!("Iter pointer overflow")
        }
    }

    fn jmp_forward(&mut self) {
        let mut jmp_stack = 0usize;
        loop {
            self.increment_pointer();
            let op = &self.ops[self.pointer];  // parser rejects unmatched skips
            match op {
                OpCode::JmpStart => {
                    jmp_stack += 1;
                }
                OpCode::JmpEnd => {
                    if jmp_stack == 0 {
                        break;
                    } else {
                        jmp_stack -= 1;
                    }
                }
                _ => {}
            }
        };
        self.increment_pointer();  // Skip the JmpEnd that we just landed on.
    }
}

impl From<Vec<OpCode>> for Interpreter {
    fn from(v: Vec<OpCode>) -> Self {
        Self::new(v)
    }
}

fn parse_from<R: Read>(reader: R) -> Result<Vec<OpCode>, String> {
    parse(reader.bytes().map(|r| r.unwrap()))
}

fn parse(buf: impl IntoIterator<Item=u8>) -> Result<Vec<OpCode>, String> {
    let mut ops = Vec::new();
    let mut open = 0usize;

    for byte in buf {
        use OpCode::*;
        let opcode = match byte {
            b'>' => MoveForward,
            b'<' => MoveBack,
            b'.' => Output,
            b',' => Input,
            b'+' => Increment,
            b'-' => Decrement,
            b'[' => {
                open += 1;
                JmpStart
            }
            b']' => {
                if open == 0 {
                    return Err("Unmatched ']'".into());
                }

                open -= 1;
                JmpEnd
            }
            _ => {continue;}
        };
        ops.push(opcode);
    };

    if open != 0 {
        Err("Unmatched '['".into())
    } else {
        Ok(ops)
    }
}

fn run_file<P: AsRef<Path>, W: Write>(path: P, writer: &mut W) {
    let ops = parse_from(File::open(path).unwrap()).unwrap();
    let mut interp: Interpreter = ops.into();
    interp.execute_all(writer);
}

fn main() {
    run_file("hello-world.bf", &mut stdout().lock());
}

#[cfg(test)]
mod test {
    // use super::*;

    #[test]
    fn test_hello_world() {
        // let mut writer = Vec::new();

    }
}