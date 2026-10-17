import { UserService } from "./user.service";

describe("UserService", () => {
  it("loads", () => {
    new UserService().load();
  });
});
