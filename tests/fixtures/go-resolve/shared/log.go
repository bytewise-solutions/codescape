package shared

func Log(s string) {
	get().OnlyInSvc()
}

func get() int { return 0 }
