*** Settings ***
Documentation       Verify that an entity can be created exclusively on a Context Source, the entity shall be retrieved correctly.

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_speed_filename}                vehicle-speed-attribute.jsonld
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D001_01_exc Create Entity With Exclusive Registration
    [Documentation]    Check that the entity is created correctly on both the Context Broker and the Context Source. The entity shall be retrieved correctly.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_1
    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    201
    ${speed_entity}=    Create Entity    ${entity_speed_filename}    ${entity_id}
    Check Response Status Code    201    ${speed_entity.status_code}

    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entities
    Should Be True    ${stub_count} > 0

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    ${body}=    Set To Dictionary    ${response.json()}
    Should Not Contain    ${body}    speed


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
