import { Component, inject, signal } from "@angular/core";
import { UserService } from "./user.service";
import { Helpers } from "../util";
import * as h from "../util/helpers";

@Component({ selector: "app-user-list" })
export class UserListComponent {
  private svc = inject(UserService);
  count = signal(0);

  async refresh(): Promise<void> {
    const user = this.svc.find("1");
    user.rename("b");
    this.svc.find("2").touch();
    this.count();
    Helpers.format(1);
    h.format2();
    unknownThing.doIt();
  }
}
