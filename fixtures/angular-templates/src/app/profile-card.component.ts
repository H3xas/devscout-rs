import { Component } from '@angular/core';

@Component({
  selector: 'app-profile-card',
  templateUrl: './profile-card.component.html',
})
export class ProfileCard {
  shown = true;
  rows: string[] = [];
  tone = 'calm';
  caption = '';
  label = '';
  selected = false;
  barWidth = 10;
  draft = '';
  greeting = 'hello';
  altText = '';
  account = { owner: { name: '' } };
  heading = '';
  currencyCode = 'EUR';
  target = 0;
  unusedFlag = false;

  rowKey(index: number): number {
    return index;
  }

  save(): void {}

  remove(at: number): void {}
}
