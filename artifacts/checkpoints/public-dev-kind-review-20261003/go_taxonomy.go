// Independent language AST oracle; no product/parser/scorer imports.
package main

import (
 "encoding/json"
 "go/ast"
 "go/parser"
 "go/token"
 "os"
)

type Definition struct {
 Name string `json:"name"`
 Kind string `json:"kind"`
 Receiver string `json:"receiver"`
 Start int `json:"start"`
 End int `json:"end"`
}

func main() {
 var source string
 if err := json.NewDecoder(os.Stdin).Decode(&source); err != nil { panic(err) }
 fs := token.NewFileSet()
 file, err := parser.ParseFile(fs, "frozen.go", source, 0)
 if err != nil { panic(err) }
 definitions := []Definition{}
 for _, declaration := range file.Decls {
  f, ok := declaration.(*ast.FuncDecl)
  if !ok { continue }
  d := Definition{Name:f.Name.Name, Kind:"function", Start:fs.Position(f.Pos()).Offset, End:fs.Position(f.End()).Offset}
  if f.Recv != nil {
   d.Kind = "method"
   t := f.Recv.List[0].Type
   if pointer, ok := t.(*ast.StarExpr); ok { t = pointer.X }
   if id, ok := t.(*ast.Ident); ok { d.Receiver = id.Name }
  }
  definitions = append(definitions, d)
 }
 if err := json.NewEncoder(os.Stdout).Encode(definitions); err != nil { panic(err) }
}
