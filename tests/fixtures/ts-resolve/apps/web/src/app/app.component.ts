import { Component, OnInit } from "@angular/core";
import { UserService } from "./users/user.service";
import { UserListComponent } from "./users/user-list.component";

@Component({ selector: "app-root", imports: [UserListComponent] })
export class AppComponent implements OnInit {
  constructor(private users: UserService) {}

  ngOnInit(): void {
    this.users.load();
  }
}
