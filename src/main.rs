//Copyright (C) 2025-  plusmouse and other contributors
//
//This program is free software: you can redistribute it and/or modify
//it under the terms of the GNU General Public License as published by
//the Free Software Foundation, either version 3 of the License, or
//(at your option) any later version.
//
//This program is distributed in the hope that it will be useful,
//but WITHOUT ANY WARRANTY; without even the implied warranty of
//MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//GNU General Public License for more details.
//
//You should have received a copy of the GNU General Public License
//along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![allow(clippy::print_stderr)]

use std::error::Error;
use std::env;

mod syntax;
mod lsp;
mod state;
mod diagnostics;
mod variables;

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "evaluate" {
        //let s = std::fs::read_to_string("../wow-ui-source/full.lua")?;
        let s = std::fs::read_to_string("tests/type-scans.lua")?;
        let mut a = syntax::syntax::Generator::new(&s);
        let numbers = line_numbers::LinePositions::from(s.as_str());
        let before = std::time::Instant::now();
        let res = a.process_all();
        let root = syntax::syntax::SyntaxNode::new_root(res.clone());
        let dur  = std::time::Instant::now() - before;
        syntax::debug::print_tree(&res);
        println!("{:#?}", res);
        println!("{:#?}", a.errors());
        //println!("{:?}", numbers.from_offset(a.errors()[0].start));
        println!("syntax: {:?}", dur);
        Ok(())
    } else {
        lsp::start_ls()
    }
}
