*** Settings ***
Documentation       Verify that a registration made without the correct operations cannot create an entity on a Context Source and shall raise an error.

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
# ETSI tool bug fixed: 4.3.6.3 requires an exclusive registration to define Attributes;
# the attribute-less vehicle-redirection-ops fixture cannot legally be exclusive.
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D001_02_exc Create Entity Without Redirection Operations
    [Documentation]    Check that the entity cannot be created in the Context Source. The creation shall raise a conflict error.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_1

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    409
    ${response}=    Create Entity    ${entity_speed_filename}    ${entity_id}
    Check Response Status Code    409    ${response.status_code}

    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entities
    Should Be True    ${stub_count} == 0


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    # ETSI tool bug fixed: the file's operations=["redirectionOps"] INCLUDES createEntity
    # (Table 4.20-2), so the broker was spec-required (5.6.1.4) to forward - contradicting
    # this test's whole premise ("without the correct operations"). Override with an
    # operations list that genuinely lacks createEntity so 5.6.1.4's "operation is not
    # supported -> error of type Conflict" applies.
    ${operations}=    Create List    queryEntity
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ...    endpoint=/broker1
    ...    operations=${operations}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
