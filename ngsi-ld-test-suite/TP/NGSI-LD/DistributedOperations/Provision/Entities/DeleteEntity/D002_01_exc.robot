*** Settings ***
Documentation       Verify that an entity can be deleted on the Context Source and the Context Broker by using an exclusive registration which supports all operations.

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
${entity_speed_filename}                vehicle-speed-attribute.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D002_01_exc Delete Entity With Exclusive Registration
    [Documentation]    Check that the entity is deleted correctly in the Context Broker and in the Context Source.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_6
    ${speed_entity}=    Load Entity    ${entity_speed_filename}    ${entity_id}

    Set Stub Reply    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}
    ${stub_count}=    Get Stub Count    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count} > 0


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
