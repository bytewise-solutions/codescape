package shared

func Log(s string) {
	get().OnlyInSvc()
}

func get() handle { return nil }
