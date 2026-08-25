*** Settings ***
Documentation       Verify that a media range weighted zero removes its representation
...                 from the offered set, and that a wildcard cannot bring it back.
...
...                 6.3.4 selects from the representations the operation offers, "unless
...                 amended by the HTTP Accept header processing rules". Those rules are
...                 IETF RFC 9110 clause 5.3.2: a media type takes its weight from the
...                 MOST SPECIFIC media range that matches it, and "a qvalue of 0 means
...                 not acceptable". So "application/json;q=0" refuses json outright even
...                 when a "*/*" in the same header would otherwise allow it, while the
...                 remaining representations stay available and decide the answer.
...
...                 Antares extension TP — 045_02 covers list order between two equally
...                 acceptable options and 045_03 an unavailable representation named
...                 alongside an available one. Neither exercises a zero weight, nor the
...                 specificity rule that decides which range a wildcard loses to.
...                 Regression guard: the wildcard was modelled as an offer OF json, so
...                 "application/json;q=0, */*" returned the very type the client refused.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create One Building
Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
045_04_01 A Zero Weight Removes Json And The Wildcard Does Not Restore It
    [Documentation]    RFC 9110 5.3.2: the exact range is the more specific match, so
    ...    json stays refused and ld+json is served
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/json;q=0, */*
    ...    application/ld+json

045_04_02 A Zero Weight Removes Json When Another Option Is Named
    [Documentation]    The refused representation is skipped and the next one decides
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/json;q=0, application/ld+json
    ...    application/ld+json

045_04_03 A Wildcard Weight Applies To Every Representation It Matches
    [Documentation]    json takes the wildcard's 0.9, which outranks ld+json's explicit
    ...    0.8, so the 6.3.4 list order breaks nothing here — the weights decide
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    */*;q=0.9, application/ld+json;q=0.8
    ...    application/json

045_04_04 An Unparseable Weight Does Not Decide The Outcome
    [Documentation]    A malformed q is not one of the RFC 9110 processing rules, so the
    ...    range keeps its default weight instead of being ranked last or first
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/ld+json;q=notanumber
    ...    application/ld+json


*** Keywords ***
Create One Building
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Retrieved Content Type Should Be
    [Arguments]    ${accept}    ${expected}
    &{headers}=    Create Dictionary    Accept=${accept}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${content_type}=    Set Variable    ${response.headers["Content-Type"]}
    Should Start With
    ...    ${content_type}
    ...    ${expected}
    ...    Accept "${accept}" must select ${expected} per 6.3.4 and RFC 9110 5.3.2
