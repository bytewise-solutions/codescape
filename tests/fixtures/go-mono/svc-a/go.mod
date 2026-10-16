module example.com/mono/svc-a

go 1.22

require (
	example.com/mono/shared v0.0.0
	github.com/gofiber/fiber/v2 v2.52.0
)

replace example.com/mono/shared => ../shared
