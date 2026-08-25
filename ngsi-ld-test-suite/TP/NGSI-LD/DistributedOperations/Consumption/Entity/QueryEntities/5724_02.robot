*** Settings ***
Documentation       Verify 5.7.2.4 filter handling on forwarded queries.
...
...                 5.7.2.4: "If split entities flag is explicitly set to
...                 true or, if not explicitly set, the default setting of
...                 the deployment allows split entities, the filters (filter
...                 conditions specified by the query, geospatial
...                 restrictions imposed by the geoquery, Scope query,
...                 Attributes) shall be removed before forwarding the
...                 request." In the non-split case (this deployment's
...                 default) the request is forwarded WITH its filters.
...
...                 Antares extension TP — the official suite never inspects
...                 the forwarded request's query string.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource
Library             Collections
Library             HttpCtrl.Server

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
5724_02_01 Non-Split Query Forwards Its Filters
    [Documentation]    5.7.2.4: with split entities OFF the q filter and the
    ...    Scope query travel with the forwarded request — the Context Source
    ...    returns its filtered subset instead of everything.
    [Tags]    dist-ops    5_7_2_4    since_v1.9.1
    &{params}=    Create Dictionary    type=Vehicle    q=speed>20    scopeQ=/A
    &{headers}=    Create Dictionary
    ...    Link=<${ngsild_test_suite_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}

    Wait For Request    ${15}
    ${forwarded_url}=    Get Request Url
    Reply By    200
    Should Contain    ${forwarded_url}    q=speed
    Should Contain    ${forwarded_url}    scopeQ=
    Should Contain    ${forwarded_url}    type=

5724_02_02 Split-Entities Query Strips Its Filters
    [Documentation]    5.7.2.4: with splitEntities=true the filters "shall be
    ...    removed before forwarding the request" — the forwarded URL must
    ...    NOT carry q or scopeQ; they are applied on the aggregate instead.
    [Tags]    dist-ops    5_7_2_4    since_v1.9.1
    &{params}=    Create Dictionary    type=Vehicle    splitEntities=true    q=speed>20    scopeQ=/A
    &{headers}=    Create Dictionary
    ...    Link=<${ngsild_test_suite_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}

    Wait For Request    ${15}
    ${forwarded_url}=    Get Request Url
    Reply By    200
    Should Contain    ${forwarded_url}    type=
    Should Not Contain    ${forwarded_url}    q=
    Should Not Contain    ${forwarded_url}    scopeQ=


*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
