*** Settings ***
Documentation       Verify that the version preference is recognised in the spellings
...                 IETF RFC 7240 permits, not only the exact lower-case bare form.
...
...                 6.3.6 carries the preference in the HTTP Prefer header, whose syntax
...                 is RFC 7240: a comma-separated list of preferences, each a token with
...                 an optional value and its own optional parameters after ";". A field
...                 name is case-insensitive per IETF RFC 9110 clause 5.6.2, so
...                 "NGSI-LD=1.0" names the same preference as "ngsi-ld=1.0", and a
...                 preference carrying "…;q=0.1" still names it.
...
...                 A broker that does not recognise the spelling silently ignores the
...                 preference: no Preference-Applied comes back, and the client cannot
...                 tell that its request was disregarded.
...
...                 Antares extension TP — 436_09 covers what the amendment does to a
...                 Subscription payload using the one bare lower-case spelling. This
...                 covers the header parsing itself, so a Subscription is used only
...                 because it answers 200 unaltered and isolates the parse.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Create One Subscription
Test Teardown       Delete Subscription    ${subscription_id}


*** Variables ***
${subscription_filename}=       subscriptions/subscription-building-entities-active-notificationAttributes.jsonld


*** Test Cases ***
436_10_01 An Upper Case Preference Name Is Honoured
    [Documentation]    RFC 9110 5.6.2: a field name is case-insensitive
    [Tags]    sub-retrieve    6_3_6    since_v1.9.1

    Preference Applied Should Be    NGSI-LD=1.0    ngsi-ld=1.0

436_10_02 A Mixed Case Preference Name Is Honoured
    [Tags]    sub-retrieve    6_3_6    since_v1.9.1

    Preference Applied Should Be    Ngsi-Ld=1.0    ngsi-ld=1.0

436_10_03 A Quoted Preference Value Is Honoured
    [Documentation]    RFC 7240: a preference value may be a token or a quoted-string
    [Tags]    sub-retrieve    6_3_6    since_v1.9.1

    Preference Applied Should Be    ngsi-ld="1.0"    ngsi-ld=1.0

436_10_04 A Preference Carrying Its Own Parameters Is Honoured
    [Documentation]    RFC 7240: parameters follow the preference after ";" and do not
    ...    change which preference it is
    [Tags]    sub-retrieve    6_3_6    since_v1.9.1

    Preference Applied Should Be    ngsi-ld=1.0;q=0.1    ngsi-ld=1.0

436_10_05 The Preference Is Found Among Other Preferences
    [Documentation]    RFC 7240: Prefer carries a comma-separated list, and the version
    ...    preference need not be the first member
    [Tags]    sub-retrieve    6_3_6    since_v1.9.1

    Preference Applied Should Be    respond-async, ngsi-ld=1.0    ngsi-ld=1.0

436_10_06 An Unrelated Preference Alone Applies No Version
    [Documentation]    Negative control: with no version preference the response must
    ...    carry no Preference-Applied at all, so the assertions above cannot pass by
    ...    the header merely always being present
    [Tags]    sub-retrieve    6_3_6    since_v1.9.1

    ${response}=    Retrieve Subscription With Prefer    ${subscription_id}    respond-async
    Check Response Status Code    200    ${response.status_code}
    ${applied}=    Evaluate    $response.headers.get('Preference-Applied')
    Should Be Equal    ${applied}    ${None}    no version was preferred, so none applies


*** Keywords ***
Create One Subscription
    ${subscription_id}=    Generate Random Subscription Id
    Set Test Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Preference Applied Should Be
    [Arguments]    ${prefer}    ${expected}
    ${response}=    Retrieve Subscription With Prefer    ${subscription_id}    ${prefer}
    Check Response Status Code    200    ${response.status_code}
    ${applied}=    Evaluate    $response.headers.get('Preference-Applied')
    Should Be Equal
    ...    ${applied}
    ...    ${expected}
    ...    Prefer "${prefer}" must be recognised as the version preference

Retrieve Subscription With Prefer
    [Arguments]    ${id}    ${prefer}
    &{headers}=    Create Dictionary    Prefer=${prefer}
    ${response}=    GET
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}
