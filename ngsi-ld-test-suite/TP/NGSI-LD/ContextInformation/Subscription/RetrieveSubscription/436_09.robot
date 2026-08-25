*** Settings ***
Documentation       Verify that the 4.3.6.8 backwards-compatibility amendment applies to
...                 Entity data only, leaving other resources untouched.
...
...                 Clause 4.3.6.8 describes the fallbacks for "NGSI-LD Entity data"
...                 (Table 4.3.6.8-1), for an NGSI-LD Property (Table 4.3.6.8-2) and for
...                 an NGSI-LD Relationship (Table 4.3.6.8-3). A Subscription (5.2.12)
...                 is none of those: none of its members has a fallback, so a request
...                 preferring an earlier version returns it verbatim — including the
...                 arrays "entities" and "notification.attributes" and the member
...                 "expiresAt", which is a Subscription member since 1.0 — and answers
...                 200, not the 203 that flags an altered payload.
...
...                 Antares extension TP — 436_08 covers the amendment itself on an
...                 Entity.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create One Subscription
Test Teardown       Delete Subscription    ${subscription_id}


*** Variables ***
${subscription_filename}=       subscriptions/subscription-building-entities-active-notificationAttributes.jsonld


*** Test Cases ***
436_09_01 Preferring An Earlier Version Leaves A Subscription Unaltered
    [Documentation]    4.3.6.8: the fallback tables cover Entity data; a Subscription
    ...    has no version fallbacks and is returned unaltered with 200
    [Tags]    sub-retrieve    4_3_6_8    6_3_6    since_v1.9.1
    ${response}=    Retrieve Subscription With Prefer    ${subscription_id}    ngsi-ld=1.0
    Check Response Status Code    200    ${response.status_code}
    ${applied}=    Evaluate    $response.headers.get('Preference-Applied')
    Should Be Equal    ${applied}    ngsi-ld=1.0
    ${type}=    Evaluate    $response.json()['type']
    Should Be Equal    ${type}    Subscription
    ${entities}=    Evaluate    $response.json()['entities']
    Should Be True    isinstance($entities, list)    entities must stay a list
    ${attributes}=    Evaluate    $response.json()['notification']['attributes']
    Should Be True    isinstance($attributes, list)    notification.attributes must stay a list
    List Should Contain Value    ${attributes}    airQualityLevel


*** Keywords ***
Create One Subscription
    ${subscription_id}=    Generate Random Subscription Id
    Set Test Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Retrieve Subscription With Prefer
    [Arguments]    ${id}    ${prefer}
    # the same @context the subscription was created under — without it the
    # response correctly compacts under the Core @context and the suite terms
    # stay expanded IRIs (6.3.5), which is not what this TP is about
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Prefer=${prefer}    Link=${context_link}
    ${response}=    GET
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}
