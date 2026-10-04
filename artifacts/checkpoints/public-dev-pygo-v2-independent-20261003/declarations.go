// Independent standard-library declaration oracle; no CodeCortex imports.
package main

import (
	"encoding/json"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
)

type Decl struct {
	Name  string `json:"name"`
	Owner string `json:"owner"`
	Kind  string `json:"kind"`
	Start int    `json:"start"`
	End   int    `json:"end"`
}

func main() {
	var s string
	if err := json.NewDecoder(os.Stdin).Decode(&s); err != nil {
		panic(err)
	}
	fs := token.NewFileSet()
	f, err := parser.ParseFile(fs, "independent.go", s, 0)
	if err != nil {
		panic(err)
	}
	out := []Decl{}
	for _, node := range f.Decls {
		switch d := node.(type) {
		case *ast.GenDecl:
			for _, sp := range d.Specs {
				name := ""
				kind := ""
				switch s := sp.(type) {
				case *ast.TypeSpec:
					name = s.Name.Name
					kind = "type"
					if _, ok := s.Type.(*ast.InterfaceType); ok {
						kind = "interface"
					}
				case *ast.ValueSpec:
					name = s.Names[0].Name
					kind = "variable"
				}
				if name != "" {
					start := sp.Pos()
					if len(d.Specs) == 1 {
						start = d.Pos()
					}
					out = append(out, Decl{name, "", kind, fs.Position(start).Offset, fs.Position(sp.End()).Offset})
				}
			}
		case *ast.FuncDecl:
			owner := ""
			kind := "function"
			if d.Recv != nil {
				kind = "method"
				t := d.Recv.List[0].Type
				if p, ok := t.(*ast.StarExpr); ok {
					t = p.X
				}
				switch r := t.(type) {
				case *ast.Ident:
					owner = r.Name
				case *ast.IndexExpr:
					owner = r.X.(*ast.Ident).Name
				case *ast.IndexListExpr:
					owner = r.X.(*ast.Ident).Name
				default:
					panic("receiver type")
				}
			}
			out = append(out, Decl{d.Name.Name, owner, kind, fs.Position(d.Pos()).Offset, fs.Position(d.End()).Offset})
		}
	}
	if err := json.NewEncoder(os.Stdout).Encode(out); err != nil {
		panic(err)
	}
}
