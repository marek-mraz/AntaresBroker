*** Settings ***
Documentation       Check the Language Filter matching rules (CIM 009 clause 4.15) the
...                 official 018_07 cases cannot distinguish: q-value ranking that
...                 contradicts list position (RFC 3282), case-insensitive langtag
...                 comparison and prefix/truncation lookup (RFC 5646), and the
...                 mandatory any-language fallback with the augmented `lang`
...                 subproperty carrying the actually returned tag.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entities
Suite Teardown      Delete Created Entities
Test Template       Retrieve Street Expecting Language


*** Variables ***
${filename}=            building-language-property.jsonld
${regional_filename}=   building-language-property-regional.jsonld


*** Test Cases ***    ENTITY    LANG_FILTER    EXPECTED_LANG    EXPECTED_VALUE
415_01_01 Q Values Outrank List Position
    [Documentation]    4.15 EXAMPLE 4 semantics: entries are ordered by quality value
    ...    (default 1), so nl;q=0.9 beats fr;q=0.2 although fr is listed first
    [Tags]    e-retrieve    4_15    since_v1.9.1
    plain    fr;q=0.2,nl;q=0.9    nl    Grote Markt

415_01_02 Langtags Compare Case-Insensitively
    [Documentation]    RFC 5646 (via 4.15): language tags are case-insensitive — FR
    ...    matches the fr languageMap key
    [Tags]    e-retrieve    4_15    since_v1.9.1
    plain    FR    fr    Grand Place

415_01_03 A Longer Range Truncates Onto A Shorter Tag
    [Documentation]    RFC 5646 lookup (via 4.15): nl-BE falls back to nl — a broker
    ...    that only knows exact matching would fall through to "any" and return fr
    [Tags]    e-retrieve    4_15    since_v1.9.1
    plain    nl-BE    nl    Grote Markt

415_01_04 A Shorter Range Matches A Longer Tag
    [Documentation]    RFC 5646 lookup (via 4.15): en matches the en-US tag — a broker
    ...    that only knows exact matching would fall through to "any" and return de
    [Tags]    e-retrieve    4_15    since_v1.9.1
    regional    en    en-US    Main Square

415_01_05 No Matching Language Defaults To Any Supported One
    [Documentation]    4.15: "If the Context Broker cannot serve any matching language,
    ...    it shall default to any supported language" — and the lang subproperty
    ...    carries the actual tag
    [Tags]    e-retrieve    4_15    since_v1.9.1
    plain    pt    ${None}    ${None}


*** Keywords ***
Retrieve Street Expecting Language
    [Arguments]    ${which}    ${lang_filter}    ${expected_lang}    ${expected_value}
    ${id}=    Set Variable If    '${which}'=='plain'    ${plain_id}    ${regional_id}
    ${response}=    Retrieve Entity
    ...    id=${id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    lang=${lang_filter}
    Check Response Status Code    200    ${response.status_code}
    ${street}=    Evaluate    $response.json()['street']
    # 4.15: the LanguageProperty is CONVERTED — Property with a string value,
    # a lang subproperty, and no languageMap left behind
    Should Be Equal    ${street['type']}    Property
    ${has_map}=    Evaluate    'languageMap' in $street
    Should Not Be True    ${has_map}    msg=languageMap must not survive the conversion
    ${actual_lang}=    Evaluate    $street.get('lang')
    IF    $expected_lang is not None
        Should Be Equal    ${actual_lang}    ${expected_lang}
        Should Be Equal    ${street['value']}    ${expected_value}
    ELSE
        # fallback case: some supported language, reported in `lang`
        Should Not Be Equal    ${actual_lang}    ${None}
        ...    msg=the augmented lang subproperty must name the returned language
    END

Create Initial Entities
    ${plain_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${plain_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${plain_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${regional_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${regional_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${regional_filename}
    ...    ${regional_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entities
    Delete Entity    ${plain_id}
    Delete Entity    ${regional_id}
