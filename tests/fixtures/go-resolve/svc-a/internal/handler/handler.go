package handler

import (
	fiber "github.com/gofiber/fiber/v2"

	"example.com/mono/svc-a/internal/repo"
)

type Service struct {
	repo repo.Repo
}

func (s *Service) Do() {
	s.repo.Get("x")
}

type H struct {
	svc *Service
}

func New(r repo.Repo) *H {
	return &H{svc: &Service{repo: r}}
}

func (h *H) Serve() {
	h.svc.Do()
}

func (h *H) RegisterRoutes(r fiber.Router) {
	r.Get("/x", h.Serve)
}
