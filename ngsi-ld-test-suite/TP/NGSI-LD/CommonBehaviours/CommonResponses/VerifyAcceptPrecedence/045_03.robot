*** Settings ***
Documentation       Verify that on an operation which does not offer application/geo+json
...                 an Accept header naming geo+json ALONGSIDE an available
...                 representation is satisfied with that representation.
...
...                 6.3.4: "the Accept header shall include (or define a media range that
...                 can be expanded to) at least one of the following options:
...                 application/json, application/ld+json, application/geo+json" and only
...                 an Accept that expands to none of the AVAILABLE representations is a
...                 406. 6.3.15 makes application/geo+json available on Retrieve Entity
...                 and Query Entities only, so elsewhere it is simply not on offer and
...                 the remaining options still decide the answer.
...
...                 Antares extension TP — 049_02 covers the geo+json-only case (406),
...                 045_02 the list order between two available options.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create One Subscription
Suite Teardown      Delete Subscription    ${subscription_id}


*** Variables ***
${subscription_filename}=       subscriptions/subscription-building-entities-active-notificationAttributes.jsonld


*** Test Cases ***
045_03_01 GeoJson Listed With Json Selects Json
    [Documentation]    application/geo+json is not available on Retrieve Subscription;
    ...    application/json is, so it is selected rather than answering 406
    [Tags]    sub-retrieve    6_3_4    6_3_15    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/geo+json, application/json
    ...    application/json

045_03_02 GeoJson Listed With LdJson Selects LdJson
    [Documentation]    the only available representation named is selected even when the
    ...    unavailable one carries the higher weight
    [Tags]    sub-retrieve    6_3_4    6_3_15    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/geo+json;q=0.9, application/ld+json;q=0.1
    ...    application/ld+json

045_03_03 Wildcard Expands To The First Option Of The List
    [Documentation]    a media range that can be expanded to the options selects the
    ...    first one of the 6.3.4 list
    [Tags]    sub-retrieve    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be    */*    application/json

045_03_04 A Zero Weighted Option Is Not Selected
    [Documentation]    "unless amended by the HTTP Accept header processing rules" —
    ...    q=0 removes an option from the offered set
    [Tags]    sub-retrieve    6_3_4    since_v1.9.1

    Retrieved Content Type Should Be
    ...    application/json;q=0, application/ld+json
    ...    application/ld+json


*** Keywords ***
Create One Subscription
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Retrieved Content Type Should Be
    [Arguments]    ${accept}    ${expected}
    ${response}=    Retrieve Subscription    id=${subscription_id}    accept=${accept}
    Check Response Status Code    200    ${response.status_code}
    ${content_type}=    Set Variable    ${response.headers["Content-Type"]}
    Should Start With
    ...    ${content_type}
    ...    ${expected}
    ...    Accept "${accept}" must select ${expected} per the 6.3.4 list order
