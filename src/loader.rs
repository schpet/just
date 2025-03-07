use super::*;
use std::collections::HashSet;

struct LinesWithEndings<'a> {
  input: &'a str,
}

impl<'a> LinesWithEndings<'a> {
  fn new(input: &'a str) -> Self {
    Self { input }
  }
}

impl<'a> Iterator for LinesWithEndings<'a> {
  type Item = &'a str;

  fn next(&mut self) -> Option<&'a str> {
    if self.input.is_empty() {
      return None;
    }
    let split = self.input.find('\n').map_or(self.input.len(), |i| i + 1);
    let (line, rest) = self.input.split_at(split);
    self.input = rest;
    Some(line)
  }
}

pub(crate) struct Loader {
  arena: Arena<String>,
  unstable: bool,
}

impl Loader {
  pub(crate) fn new(unstable: bool) -> Self {
    Loader {
      arena: Arena::new(),
      unstable,
    }
  }

  pub(crate) fn load<'src>(&'src self, path: &Path) -> RunResult<&'src str> {
    let src = self.load_recursive(path, HashSet::new())?;
    Ok(self.arena.alloc(src))
  }

  fn load_file<'a>(path: &Path) -> RunResult<'a, String> {
    fs::read_to_string(path).map_err(|io_error| Error::Load {
      path: path.to_owned(),
      io_error,
    })
  }

  fn load_recursive(&self, file: &Path, seen: HashSet<PathBuf>) -> RunResult<String> {
    let src = Self::load_file(file)?;

    let mut output = String::new();

    let mut seen_content = false;

    for (i, line) in LinesWithEndings::new(&src).enumerate() {
      if !seen_content && line.starts_with('!') {
        let include = line
          .strip_prefix("!include")
          .ok_or_else(|| Error::InvalidDirective { line: line.into() })?;

        if !self.unstable {
          return Err(Error::Unstable {
            message: "The !include directive is currently unstable.".into(),
          });
        }

        let argument = include.trim();

        if argument.is_empty() {
          return Err(Error::IncludeMissingPath {
            file: file.to_owned(),
            line: i,
          });
        }

        let contents = self.process_include(file, Path::new(argument), &seen)?;

        output.push_str(&contents);
      } else {
        if !(line.trim().is_empty() || line.trim().starts_with('#')) {
          seen_content = true;
        }
        output.push_str(line);
      }
    }

    Ok(output)
  }

  fn process_include(
    &self,
    file: &Path,
    include: &Path,
    seen: &HashSet<PathBuf>,
  ) -> RunResult<String> {
    let canonical_path = if include.is_relative() {
      let current_dir = file.parent().ok_or(Error::Internal {
        message: format!(
          "Justfile path `{}` has no parent directory",
          include.display()
        ),
      })?;
      current_dir.join(include)
    } else {
      include.to_owned()
    };

    let canonical_path = canonical_path.lexiclean();

    if seen.contains(&canonical_path) {
      return Err(Error::CircularInclude {
        current: file.to_owned(),
        include: canonical_path,
      });
    }

    let mut seen_paths = seen.clone();
    seen_paths.insert(file.lexiclean());

    self.load_recursive(&canonical_path, seen_paths)
  }
}

#[cfg(test)]
mod tests {
  use super::{Error, Lexiclean, Loader};
  use temptree::temptree;

  #[test]
  fn include_justfile() {
    let justfile_a = r#"
# A comment at the top of the file
!include ./justfile_b

some_recipe: recipe_b
    echo "some recipe"
"#;

    let justfile_b = r#"!include ./subdir/justfile_c

recipe_b: recipe_c
    echo "recipe b"
"#;

    let justfile_c = r#"recipe_c:
    echo "recipe c"
"#;

    let tmp = temptree! {
        justfile: justfile_a,
        justfile_b: justfile_b,
        subdir: {
            justfile_c: justfile_c
        }
    };

    let full_concatenated_output = r#"
# A comment at the top of the file
recipe_c:
    echo "recipe c"

recipe_b: recipe_c
    echo "recipe b"

some_recipe: recipe_b
    echo "some recipe"
"#;

    let loader = Loader::new(true);

    let justfile_a_path = tmp.path().join("justfile");
    let loader_output = loader.load(&justfile_a_path).unwrap();

    assert_eq!(loader_output, full_concatenated_output);
  }

  #[test]
  fn recursive_includes_fail() {
    let justfile_a = r#"
# A comment at the top of the file
!include ./subdir/justfile_b

some_recipe: recipe_b
    echo "some recipe"

"#;

    let justfile_b = r#"
!include ../justfile

recipe_b:
    echo "recipe b"
"#;
    let tmp = temptree! {
        justfile: justfile_a,
        subdir: {
            justfile_b: justfile_b
        }
    };

    let loader = Loader::new(true);

    let justfile_a_path = tmp.path().join("justfile");
    let loader_output = loader.load(&justfile_a_path).unwrap_err();

    assert_matches!(loader_output, Error::CircularInclude { current, include }
        if current == tmp.path().join("subdir").join("justfile_b").lexiclean() &&
        include == tmp.path().join("justfile").lexiclean()
    );
  }
}
