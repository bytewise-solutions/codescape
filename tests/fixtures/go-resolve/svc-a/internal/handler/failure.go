package handler

type Failure struct{}

func (f *Failure) Error() string { return "failure" }

func load() error { return nil }

func Report(err error) string {
	return err.Error()
}

func Reload() string {
	e := load()
	return e.Error()
}
