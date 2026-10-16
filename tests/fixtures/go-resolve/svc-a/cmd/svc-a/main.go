package main

import (
	"example.com/mono/shared"
	"example.com/mono/svc-a/internal/handler"
	"example.com/mono/svc-a/internal/repo"
)

func main() {
	shared.Log("start")
	r := repo.NewPgRepo()
	h := handler.New(r)
	h.Serve()
	println(len("x"))
}
