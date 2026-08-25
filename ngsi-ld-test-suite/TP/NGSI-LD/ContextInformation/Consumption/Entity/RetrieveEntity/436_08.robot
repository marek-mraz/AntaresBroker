*** Settings ***
Documentation       Verify the 4.3.6.8 backwards-compatibility amendment of payloads.
...
...                 Clause 4.3.6.8: a consumer "may wish to indicate that it is only
...                 capable of interpreting responses which conform to a specific
...                 NGSI-LD specification, in which case the Context Source shall
...                 endeavour to amend its payload accordingly." Table 4.3.6.8-1:
...                 VocabProperty (introduced 1.8) falls back to "Reformat attribute
...                 as Property" for earlier versions. The HTTP binding carries the
...                 request as Prefer: ngsi-ld=<major.minor> (RFC 7240), answered
...                 with Preference-Applied and 203 when the payload was altered.
...
...                 Antares extension TP — no official TP exercises the conformance
...                 fallback tables.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Building With VocabProperty
Test Teardown       Delete Entity    ${entity_id}


*** Variables ***
${entity_payload_filename}      building-vocab-property-string.jsonld


*** Test Cases ***
436_08_01 Preferring An Earlier Version Reformats A VocabProperty As Property
    [Documentation]    4.3.6.8 Table 4.3.6.8-1: VocabProperty was introduced in 1.8;
    ...    a consumer preferring 1.5 receives it reformatted as a Property, with
    ...    Preference-Applied echoing the version and 203 flagging the altered payload.
    [Tags]    e-retrieve    4_3_6_8    since_v1.9.1
    ${response}=    Retrieve Entity With Prefer    ${entity_id}    ngsi-ld=1.5
    Check Response Status Code    203    ${response.status_code}
    ${applied}=    Evaluate    $response.headers.get('Preference-Applied')
    Should Be Equal    ${applied}    ngsi-ld=1.5
    ${attr_type}=    Evaluate    $response.json()['vocabProperty']['type']
    Should Be Equal    ${attr_type}    Property

436_08_02 Preferring The Native Version Leaves The Payload Untouched
    [Documentation]    4.3.6.8: a payload already conformant to the preferred version
    ...    is returned unaltered (200) with Preference-Applied confirming the version.
    [Tags]    e-retrieve    4_3_6_8    since_v1.9.1
    ${response}=    Retrieve Entity With Prefer    ${entity_id}    ngsi-ld=1.9
    Check Response Status Code    200    ${response.status_code}
    ${applied}=    Evaluate    $response.headers.get('Preference-Applied')
    Should Be Equal    ${applied}    ngsi-ld=1.9
    ${attr_type}=    Evaluate    $response.json()['vocabProperty']['type']
    Should Be Equal    ${attr_type}    VocabProperty


*** Keywords ***
Create Building With VocabProperty
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

Retrieve Entity With Prefer
    [Arguments]    ${id}    ${prefer}
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Link=${context_link}    Prefer=${prefer}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}
