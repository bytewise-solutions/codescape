import { Injectable, inject } from "@angular/core";
import { HttpClient } from "@angular/common/http";
import { log, Store } from "@demo/shared";
import { User } from "@app/users/user";
import { BaseService } from "./base.service";

@Injectable({ providedIn: "root" })
export class UserService extends BaseService {
  private http = inject(HttpClient);
  private store!: Store;

  load(): Promise<User[]> {
    log("loading");
    this.http.get("/api/users");
    this.store.get("1");
    this.base();
    return Promise.resolve([]);
  }

  find(id: string): User {
    const u = new User(id);
    u.rename("a");
    return u;
  }
}
