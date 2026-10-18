import { Component } from '@angular/core';

const SHARED_PATH = './shared/badge-view.html';
const MARKUP = '<p>shared</p>';

@Component({
  selector: 'app-missing-template',
  templateUrl: './missing-template.html',
})
export class MissingTemplate {
  lostField = '';
}

@Component({
  selector: 'app-computed-template',
  templateUrl: SHARED_PATH,
})
export class ComputedTemplate {
  computedField = '';
}

@Component({
  selector: 'app-substituted-template',
  template: `<div>${MARKUP}</div>`,
})
export class SubstitutedTemplate {
  substitutedField = '';
}
