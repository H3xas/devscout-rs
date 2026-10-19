import { Component } from '@angular/core';

class Tracker {}

@Component({
  selector: 'app-member-kinds',
  template: `
    <p>{{ expanded }}</p>
    <button (click)="toggle()">toggle</button>
    <p>{{ ownerName }}</p>
    <p>{{ registry }}</p>
  `,
})
export class MemberKinds {
  static registry = 1;

  get expanded(): boolean {
    return true;
  }

  set expanded(value: boolean) {}

  toggle(): void {}

  constructor(
    private readonly tracker: Tracker,
    public ownerName: string,
    plainArgument: number,
  ) {}
}
