*** Settings ***
Documentation       Verify that when two redirect registrations are configured on a Context Broker, it is possible to create entities in both Context Sources, but not directly in the Context Broker

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Entity Id And Registration And Start Context Source Mock Server
Test Teardown       Delete Registrations And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld


*** Test Cases ***
D001_01_red Create Entity On Both Context Sources
    [Documentation]    Check that if one requests the Context Broker to create an entity that matches two redirect registrations, this is created only on the Context Sources
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxied-redirect    4_3_6_3    5_6_1
    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    201
    Set Stub Reply    POST    /broker2/ngsi-ld/v1/entities    201

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entities
    Should Be True    ${stub_count} > 0

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id2}
    Check Response Status Code    201    ${response.status_code}
    ${stub_count}=    Get Stub Count    POST    /broker2/ngsi-ld/v1/entities
    Should Be True    ${stub_count} > 0

*** Keywords ***
Setup Entity Id And Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}    
    ${entity_id2}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id2}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id2}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response2}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response2.status_code}
    
    Start Context Source Mock Server
Delete Registrations And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Delete Entity    ${entity_id}
    Delete Entity    ${entity_id2}
    Stop Context Source Mock Server