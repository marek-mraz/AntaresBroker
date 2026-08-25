*** Settings ***
Documentation       Verify that when an Accept header expands to more than one supported
...                 representation, the one selected follows the specification's list
...                 order rather than the order the client wrote the tokens in.
...
...                 6.3.4: "Accept header shall include (or define a media range that can
...                 be expanded to) at least one of: application/json, application/ld+json,
...                 application/geo+json. The order of the list above is significant. If
...                 the Accept header can be expanded to more than one of the options of
...                 the list, the first one of the list shall be selected, unless amended
...                 by the HTTP Accept header processing rules, e.g. the presence of a
...                 \"q\" parameter indicating a relative weight."
...
...                 Antares extension TP — 045_01 covers only the absent-Accept case.
...                 Regression guard: the tie-break used header order, so
...                 "application/ld+json, application/json" wrongly returned ld+json.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create One Building
Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
045_02_01 Accept Listing LdJson Before Json Selects Json
    [Documentation]    Equal weights — list order wins, so application/json is selected
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/ld+json, application/json
    ...    application/json

045_02_02 Accept Listing GeoJson Before Json Selects Json
    [Documentation]    application/json precedes application/geo+json in the 6.3.4 list
    [Tags]    common-behaviours    6_3_4    6_3_15    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/geo+json, application/json
    ...    application/json

045_02_03 Quality Values Override List Order
    [Documentation]    "…unless amended by the HTTP Accept header processing rules, e.g.
    ...    the presence of a q parameter" — an explicit weight still wins
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/json;q=0.1, application/ld+json;q=0.9
    ...    application/ld+json

045_02_04 A Single Supported Type Is Honoured
    [Documentation]    With only one option present there is no tie to break
    [Tags]    common-behaviours    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/ld+json
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
    ...    Accept "${accept}" must select ${expected} per the 6.3.4 list order
