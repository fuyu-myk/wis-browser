import type { Address } from '@/bindings/types'

const COUNTRY_CODES = (
  'AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR ' +
  'BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ ' +
  'EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW ' +
  'GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY ' +
  'KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV ' +
  'MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY ' +
  'QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG ' +
  'TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW'
).split(' ')

export function emptyAddress(): Address {
  return {
    given_name: '',
    family_name: '',
    organization: '',
    street_address: '',
    city: '',
    region: '',
    postal_code: '',
    country: '',
    tel: '',
    email: '',
  }
}

function regionName(code: string, locale?: string): string {
  try {
    return new Intl.DisplayNames(locale ? [locale] : undefined, { type: 'region' }).of(code) ?? code
  } catch {
    return code
  }
}

export interface CountryOption {
  code: string
  name: string
}

export function countryOptions(locale?: string): CountryOption[] {
  const collator = new Intl.Collator(locale)
  return COUNTRY_CODES.map((code) => ({ code, name: regionName(code, locale) })).sort((a, b) =>
    collator.compare(a.name, b.name),
  )
}

/** A saved address as a list row. */
export function addressSummary(address: Address, locale?: string): { title: string; detail: string } {
  const street = address.street_address.split('\n')[0]?.trim() ?? ''
  const name = [address.given_name, address.family_name].filter(Boolean).join(' ')
  const title =
    name || address.organization || street || address.city || address.email || address.tel || 'Address'
  const detail = [
    title === street ? '' : street,
    title === address.city ? '' : address.city,
    address.region,
    address.postal_code,
    address.country ? regionName(address.country, locale) : '',
  ]
    .filter(Boolean)
    .join(', ')
  return { title, detail }
}

/** Whether `query` appears in any field of `address` or its country's name. */
export function addressMatches(address: Address, query: string, locale?: string): boolean {
  const wanted = query.trim().toLocaleLowerCase(locale)
  if (!wanted) return true
  const country = address.country ? regionName(address.country, locale) : ''
  return [...Object.values(address), country].some((field) =>
    field.toLocaleLowerCase(locale).includes(wanted),
  )
}
