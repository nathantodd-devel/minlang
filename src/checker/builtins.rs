//! Names the language provides without any declaration, including the
//! built-in modules `std` and `types`. Other modules are files; see
//! `crate::modules`.

use super::types::{IntType, Type};

pub const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64", "usize", "s8", "s16", "s32", "s64", "f32", "f64", "bool",
];

pub fn primitive(name: &str) -> Option<Type> {
    Some(match name {
        "u8" => Type::Int(IntType::new(false, 8)),
        "u16" => Type::Int(IntType::new(false, 16)),
        "u32" => Type::Int(IntType::new(false, 32)),
        "u64" => Type::Int(IntType::new(false, 64)),
        "usize" => Type::Int(IntType::USIZE),
        "s8" => Type::Int(IntType::new(true, 8)),
        "s16" => Type::Int(IntType::new(true, 16)),
        "s32" => Type::Int(IntType::new(true, 32)),
        "s64" => Type::Int(IntType::new(true, 64)),
        "f32" => Type::Float(32),
        "f64" => Type::Float(64),
        "bool" => Type::Bool,
        _ => return None,
    })
}

/// A C library function. Its symbol is its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinFunction {
    pub name: &'static str,
    pub params: Vec<Type>,
    /// Whether it takes further arguments after `params`, like `printf`.
    pub variadic: bool,
    pub return_type: Option<Type>,
}

/// A C type in a libc signature, as it is on 64-bit Linux.
#[derive(Debug, Clone, Copy)]
enum C {
    Void,
    /// `int`, and `char` passed as an `int` (`putchar`, `isalpha`), `pid_t`
    Int,
    /// `unsigned int`, `mode_t`
    UInt,
    /// `long`, `long long`, `ssize_t`, `off_t`, `time_t`, `clock_t`
    Long,
    /// `unsigned long`, `unsigned long long`
    ULong,
    /// `size_t`
    Size,
    Float,
    Double,
    /// `char *`
    Str,
    /// Any other pointer: `void *`, `FILE *`, `char **`
    Ptr,
}

impl C {
    fn ty(self) -> Option<Type> {
        Some(match self {
            C::Void => return None,
            C::Int => Type::Int(IntType::S32),
            C::UInt => Type::Int(IntType::new(false, 32)),
            C::Long => Type::Int(IntType::new(true, 64)),
            C::ULong => Type::Int(IntType::new(false, 64)),
            C::Size => Type::Int(IntType::USIZE),
            C::Float => Type::Float(32),
            C::Double => Type::Float(64),
            C::Str => Type::Array {
                element: Box::new(Type::Int(IntType::new(false, 8))),
                size: None,
            },
            C::Ptr => Type::void_pointer(),
        })
    }
}

/// Name, return type, parameters, and whether it's variadic.
type Signature = (&'static str, C, &'static [C], bool);

/// The C library functions `std` provides. Ones that take function pointers
/// (`qsort`, `atexit`) or a `va_list` can't be written in minlang's types.
#[rustfmt::skip]
const LIBC: &[Signature] = {
    use C::*;
    &[
        // stdio.h
        ("printf", Int, &[Str], true),
        ("fprintf", Int, &[Ptr, Str], true),
        ("dprintf", Int, &[Int, Str], true),
        ("sprintf", Int, &[Str, Str], true),
        ("snprintf", Int, &[Str, Size, Str], true),
        ("scanf", Int, &[Str], true),
        ("fscanf", Int, &[Ptr, Str], true),
        ("sscanf", Int, &[Str, Str], true),
        ("puts", Int, &[Str], false),
        ("fputs", Int, &[Str, Ptr], false),
        ("putchar", Int, &[Int], false),
        ("fputc", Int, &[Int, Ptr], false),
        ("putc", Int, &[Int, Ptr], false),
        ("getchar", Int, &[], false),
        ("fgetc", Int, &[Ptr], false),
        ("getc", Int, &[Ptr], false),
        ("ungetc", Int, &[Int, Ptr], false),
        ("fgets", Str, &[Str, Int, Ptr], false),
        ("getline", Long, &[Ptr, Ptr, Ptr], false),
        ("fopen", Ptr, &[Str, Str], false),
        ("fdopen", Ptr, &[Int, Str], false),
        ("freopen", Ptr, &[Str, Str, Ptr], false),
        ("fclose", Int, &[Ptr], false),
        ("fflush", Int, &[Ptr], false),
        ("fread", Size, &[Ptr, Size, Size, Ptr], false),
        ("fwrite", Size, &[Ptr, Size, Size, Ptr], false),
        ("fseek", Int, &[Ptr, Long, Int], false),
        ("ftell", Long, &[Ptr], false),
        ("rewind", Void, &[Ptr], false),
        ("feof", Int, &[Ptr], false),
        ("ferror", Int, &[Ptr], false),
        ("clearerr", Void, &[Ptr], false),
        ("fileno", Int, &[Ptr], false),
        ("setvbuf", Int, &[Ptr, Str, Int, Size], false),
        ("tmpfile", Ptr, &[], false),
        ("perror", Void, &[Str], false),
        ("remove", Int, &[Str], false),
        ("rename", Int, &[Str, Str], false),
        // stdlib.h
        ("malloc", Ptr, &[Size], false),
        ("calloc", Ptr, &[Size, Size], false),
        ("realloc", Ptr, &[Ptr, Size], false),
        ("aligned_alloc", Ptr, &[Size, Size], false),
        ("free", Void, &[Ptr], false),
        ("exit", Void, &[Int], false),
        ("_Exit", Void, &[Int], false),
        ("abort", Void, &[], false),
        ("atoi", Int, &[Str], false),
        ("atol", Long, &[Str], false),
        ("atoll", Long, &[Str], false),
        ("atof", Double, &[Str], false),
        ("strtol", Long, &[Str, Ptr, Int], false),
        ("strtoll", Long, &[Str, Ptr, Int], false),
        ("strtoul", ULong, &[Str, Ptr, Int], false),
        ("strtoull", ULong, &[Str, Ptr, Int], false),
        ("strtod", Double, &[Str, Ptr], false),
        ("strtof", Float, &[Str, Ptr], false),
        ("abs", Int, &[Int], false),
        ("labs", Long, &[Long], false),
        ("llabs", Long, &[Long], false),
        ("rand", Int, &[], false),
        ("srand", Void, &[UInt], false),
        ("getenv", Str, &[Str], false),
        ("setenv", Int, &[Str, Str, Int], false),
        ("unsetenv", Int, &[Str], false),
        ("system", Int, &[Str], false),
        // string.h
        ("strlen", Size, &[Str], false),
        ("strnlen", Size, &[Str, Size], false),
        ("strcmp", Int, &[Str, Str], false),
        ("strncmp", Int, &[Str, Str, Size], false),
        ("strcasecmp", Int, &[Str, Str], false),
        ("strncasecmp", Int, &[Str, Str, Size], false),
        ("strcpy", Str, &[Str, Str], false),
        ("strncpy", Str, &[Str, Str, Size], false),
        ("strcat", Str, &[Str, Str], false),
        ("strncat", Str, &[Str, Str, Size], false),
        ("strchr", Str, &[Str, Int], false),
        ("strrchr", Str, &[Str, Int], false),
        ("strstr", Str, &[Str, Str], false),
        ("strpbrk", Str, &[Str, Str], false),
        ("strspn", Size, &[Str, Str], false),
        ("strcspn", Size, &[Str, Str], false),
        ("strtok", Str, &[Str, Str], false),
        ("strdup", Str, &[Str], false),
        ("strndup", Str, &[Str, Size], false),
        ("strerror", Str, &[Int], false),
        ("memcpy", Ptr, &[Ptr, Ptr, Size], false),
        ("memmove", Ptr, &[Ptr, Ptr, Size], false),
        ("memset", Ptr, &[Ptr, Int, Size], false),
        ("memcmp", Int, &[Ptr, Ptr, Size], false),
        ("memchr", Ptr, &[Ptr, Int, Size], false),
        // ctype.h
        ("isalnum", Int, &[Int], false),
        ("isalpha", Int, &[Int], false),
        ("iscntrl", Int, &[Int], false),
        ("isdigit", Int, &[Int], false),
        ("isgraph", Int, &[Int], false),
        ("islower", Int, &[Int], false),
        ("isprint", Int, &[Int], false),
        ("ispunct", Int, &[Int], false),
        ("isspace", Int, &[Int], false),
        ("isupper", Int, &[Int], false),
        ("isxdigit", Int, &[Int], false),
        ("tolower", Int, &[Int], false),
        ("toupper", Int, &[Int], false),
        // math.h
        ("sqrt", Double, &[Double], false), ("sqrtf", Float, &[Float], false),
        ("cbrt", Double, &[Double], false), ("cbrtf", Float, &[Float], false),
        ("pow", Double, &[Double, Double], false), ("powf", Float, &[Float, Float], false),
        ("exp", Double, &[Double], false), ("expf", Float, &[Float], false),
        ("exp2", Double, &[Double], false), ("exp2f", Float, &[Float], false),
        ("log", Double, &[Double], false), ("logf", Float, &[Float], false),
        ("log2", Double, &[Double], false), ("log2f", Float, &[Float], false),
        ("log10", Double, &[Double], false), ("log10f", Float, &[Float], false),
        ("sin", Double, &[Double], false), ("sinf", Float, &[Float], false),
        ("cos", Double, &[Double], false), ("cosf", Float, &[Float], false),
        ("tan", Double, &[Double], false), ("tanf", Float, &[Float], false),
        ("asin", Double, &[Double], false), ("asinf", Float, &[Float], false),
        ("acos", Double, &[Double], false), ("acosf", Float, &[Float], false),
        ("atan", Double, &[Double], false), ("atanf", Float, &[Float], false),
        ("atan2", Double, &[Double, Double], false), ("atan2f", Float, &[Float, Float], false),
        ("sinh", Double, &[Double], false), ("sinhf", Float, &[Float], false),
        ("cosh", Double, &[Double], false), ("coshf", Float, &[Float], false),
        ("tanh", Double, &[Double], false), ("tanhf", Float, &[Float], false),
        ("floor", Double, &[Double], false), ("floorf", Float, &[Float], false),
        ("ceil", Double, &[Double], false), ("ceilf", Float, &[Float], false),
        ("round", Double, &[Double], false), ("roundf", Float, &[Float], false),
        ("trunc", Double, &[Double], false), ("truncf", Float, &[Float], false),
        ("fabs", Double, &[Double], false), ("fabsf", Float, &[Float], false),
        ("fmod", Double, &[Double, Double], false), ("fmodf", Float, &[Float, Float], false),
        ("hypot", Double, &[Double, Double], false), ("hypotf", Float, &[Float, Float], false),
        ("fmin", Double, &[Double, Double], false), ("fminf", Float, &[Float, Float], false),
        ("fmax", Double, &[Double, Double], false), ("fmaxf", Float, &[Float, Float], false),
        // unistd.h, fcntl.h, sys/stat.h
        ("read", Long, &[Int, Ptr, Size], false),
        ("write", Long, &[Int, Ptr, Size], false),
        ("open", Int, &[Str, Int], true),
        ("close", Int, &[Int], false),
        ("lseek", Long, &[Int, Long, Int], false),
        ("unlink", Int, &[Str], false),
        ("access", Int, &[Str, Int], false),
        ("getcwd", Str, &[Str, Size], false),
        ("chdir", Int, &[Str], false),
        ("mkdir", Int, &[Str, UInt], false),
        ("rmdir", Int, &[Str], false),
        ("isatty", Int, &[Int], false),
        ("getpid", Int, &[], false),
        ("getppid", Int, &[], false),
        ("fork", Int, &[], false),
        ("sleep", UInt, &[UInt], false),
        ("usleep", Int, &[UInt], false),
        // time.h
        ("time", Long, &[Ptr], false),
        ("clock", Long, &[], false),
    ]
};

fn builtin_function(name: &str) -> Option<BuiltinFunction> {
    let &(name, return_type, params, variadic) =
        LIBC.iter().find(|signature| signature.0 == name)?;
    Some(BuiltinFunction {
        name,
        params: params.iter().filter_map(|param| param.ty()).collect(),
        variadic,
        return_type: return_type.ty(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinMember {
    Function(BuiltinFunction),
    Type(Type),
}

pub struct Module {
    pub name: &'static str,
    /// Whether it exports the C library functions, like `std`.
    exports_libc: bool,
    /// Whether the module exports the built-in types, like `types`.
    exports_primitives: bool,
}

impl Module {
    pub fn member(&self, name: &str) -> Option<BuiltinMember> {
        if self.exports_libc {
            return builtin_function(name).map(BuiltinMember::Function);
        }
        if self.exports_primitives {
            return primitive(name).map(BuiltinMember::Type);
        }
        None
    }

    pub fn members(&self) -> Vec<&'static str> {
        let mut members = Vec::new();
        if self.exports_libc {
            members.extend(LIBC.iter().map(|signature| signature.0));
        }
        if self.exports_primitives {
            members.extend(PRIMITIVES);
        }
        members
    }
}

const MODULES: &[Module] = &[
    Module {
        name: "std",
        exports_libc: true,
        exports_primitives: false,
    },
    Module {
        name: "types",
        exports_libc: false,
        exports_primitives: true,
    },
];

pub fn module(name: &str) -> Option<&'static Module> {
    MODULES.iter().find(|module| module.name == name)
}
