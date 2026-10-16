package shared

type Hook interface {
	Fire()
}

type Bus struct {
	h Hook
}

func (b *Bus) Run() {
	b.h.Fire()
}
