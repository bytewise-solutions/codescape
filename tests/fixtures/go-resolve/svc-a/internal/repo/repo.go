package repo

type Repo interface {
	Get(id string) string
}

type pgRepo struct{}

func (p *pgRepo) Get(id string) string {
	return id
}

func NewPgRepo() Repo {
	return &pgRepo{}
}
