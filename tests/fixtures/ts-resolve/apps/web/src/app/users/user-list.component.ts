import { Component, inject } from "@angular/core";
import { UserService } from "./user.service";
import { Helpers } from "../util";
import * as h from "../util/helpers";

@Component({ selector: "app-user-list" })
export class UserListComponent {
  private svc = inject(UserService);

  async refresh(): Promise<void> {
    const user = this.svc.find("1");
    user.rename("b");
    Helpers.format(1);
    h.format2();
    unknownThing.doIt();
  }
}
