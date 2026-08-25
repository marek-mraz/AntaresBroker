*** Settings ***
Documentation       Check that the numberOfHits is increased after using a Cached context

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/ContextServerUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Library             Collections
Library             String
Variables           ${EXECDIR}/resources/variables.py
Library             HttpCtrl.Client
Library             HttpCtrl.Server

Test Setup          Create Initial @context condition from an external server
Test Teardown       Delete Initial @context condition from an external server


*** Variables ***
${filename}=                @context-minimal-valid.json
${entityfile}=              minimal-entity-using-@context.jsonld
${uri}                      /api/v1/context.jsonld
${reason_200}=              OK
${testing_id_prefix}=       urn:ngsi-ld:Testing:


*** Test Cases ***
053_06_01 Check That The numberOfHits Is Increased After Using A Cached Context
    [Documentation]    Check that the numberOfHits is increased after using a Cached context
    [Tags]    ctx-serve    5_13_4    since_v1.5.1

    ${second_existing_entity_id}=    Generate Random Id    ${testing_id_prefix}
    Set Global Variable    ${first_existing_entity_id}

    Create Entity selecting @context    ${entityfile}    ${uri}    ${second_existing_entity_id}

    ${response}=    Serve a @context
    ...    contextId=${uri}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Context Response Body Containing numberOfHits value    ${response.json()}    3


*** Keywords ***
Create Initial @context condition from an external server
    Start @context Local Server

    ${first_existing_entity_id}=    Generate Random Id    ${testing_id_prefix}
    Set Global Variable    ${first_existing_entity_id}

    # Reset ${uri} to the relative path first: a prior mock test does "Set Global Variable ${uri}"
    # with the already-absolute URL, which leaks into this test and makes the Catenate below double
    # the host (http://host:porthttp://host:port/...). Passing in isolation, failing in the full
    # suite, was caused purely by that leaked global.
    ${uri}=    Set Variable    /api/v1/context.jsonld
    ${uri}=    Catenate    SEPARATOR=    http://${context_server_host}:${context_server_port}    ${uri}
    Set Global Variable    ${uri}

    Create Entity selecting @context    ${entityfile}    ${uri}    ${first_existing_entity_id}
    ${response}=    Serve a @context
    ...    contextId=${uri}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Context Response Body Containing numberOfHits value    ${response.json()}    1

Delete Initial @context condition from an external server
    Log    Delete initial contidions
    Delete Entity    ${first_existing_entity_id}
    Delete a @context    ${uri}
    Stop @context Local Server
