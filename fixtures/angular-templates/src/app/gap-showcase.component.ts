import { Component } from '@angular/core';
import { ShowcaseBase } from './showcase-base';

@Component({
  selector: 'app-gap-showcase',
  templateUrl: './gap-showcase.component.html',
})
export class GapShowcase extends ShowcaseBase {
  ready = false;
  insideBlock = '';
  firstTerm = '';
}
