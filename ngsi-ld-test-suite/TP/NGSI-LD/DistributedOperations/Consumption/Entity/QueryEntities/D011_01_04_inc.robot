*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker, an entity exists on both
...                 the Context Broker and a Context Source, and one queries for an attribute absent from both entities,
...                 an empty result is returned

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename2}             vehicle-simple-attributes-second.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D011_01_04_inc Query The Context Broker With Type And Attribute In Neither
    [Documentation]    Check that if one queries for attribute present in neither of the entities, neither of them gets
    ...    returned. Contrast with D011_01_02, where no local entity exists at all.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_2
    ${empty_response}=    Create Empty Array Result
    Set Stub Reply    GET    /ngsi-ld/v1/entities?attrs=speed&type=Vehicle    200    ${empty_response}

    ${response}=    Query Entities    entity_types=Vehicle    attrs=speed    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Should Be Empty    ${response.json()}


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${response1}=    Create Entity    ${entity_payload_filename2}    ${entity_id}
    Check Response Status Code    201    ${response1.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ${response2}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response2.status_code}
    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
