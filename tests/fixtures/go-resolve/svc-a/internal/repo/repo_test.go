package repo

type fakeRepo struct{}

func (f *fakeRepo) Get(id string) string {
	return ""
}
